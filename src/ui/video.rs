use windows::{
    Win32::{
        Foundation::{POINT, RECT, SIZE},
        Graphics::{
            Direct2D::{
                Common::D2D1_COLOR_F, D2D1_DRAW_TEXT_OPTIONS_CLIP, ID2D1DCRenderTarget,
                ID2D1SolidColorBrush,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
                DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
                IDWriteFactory, IDWriteTextFormat,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

use crate::{
    i18n::Language,
    platform::playback::{Phase, PlaybackStatus, Preferences},
};

use super::{layout::PxRect, theme::rgb};

mod edge;
mod layer;
pub use edge::{EdgeLayout, EdgeZone, ProgressLine, VolumeHit, VolumeLayout, VolumePainter};
pub use layer::{Layer, LayerKind, WM_PANEL_INPUT, WM_VOLUME_INPUT};

const ACCENT: D2D1_COLOR_F = rgb(0xff8d42);
const HOT_ACCENT: D2D1_COLOR_F = rgb(0xffb277);
const INK: D2D1_COLOR_F = rgb(0xfff4eb);
const MUTED: D2D1_COLOR_F = rgb(0xc9afa0);
const TRACK: D2D1_COLOR_F = rgb(0x503b30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Previous,
    Play,
    Next,
    Volume,
    Loop,
    Rate,
    Fullscreen,
    Open,
    Update,
    More,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Button(Action),
    Seek,
}

#[derive(Clone, PartialEq)]
pub struct PanelLayout {
    pub offset: POINT,
    pub size: SIZE,
    scale: f32,
    buttons: Vec<(Action, PxRect)>,
    seek: PxRect,
    time: PxRect,
    hint: PxRect,
}

impl PanelLayout {
    pub fn new(client: RECT, dpi_scale: f32, update: bool) -> Option<Self> {
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width < 100 || height < 60 {
            return None;
        }
        let scale = dpi_scale.min(height as f32 / 112.0);
        let inset = if width as f32 >= 350.0 * scale {
            14.0 * scale
        } else {
            6.0 * scale
        };
        let size = SIZE {
            cx: (width as f32 - 2.0 * inset).min(916.0 * scale).round() as i32,
            cy: (94.0 * scale).round() as i32,
        };
        let width = size.cx as f32;
        let offset = POINT {
            x: client.left + (client.right - client.left - size.cx) / 2,
            y: client.bottom - size.cy - (inset.round() as i32),
        };
        let pad = if width >= 270.0 * scale { 14.0 } else { 8.0 } * scale;
        let right = width - pad;
        let seek = PxRect::new(pad, 47.0 * scale, right, 73.0 * scale);
        let time = PxRect::new(pad, 71.0 * scale, right, 91.0 * scale);
        let hint_margin = (62.0 * scale).min((right - pad) * 0.25);
        let hint = PxRect::new(
            pad + hint_margin,
            time.top,
            right - hint_margin,
            time.bottom,
        );
        let primary = [Action::Previous, Action::Play, Action::Next];
        let mut secondary = vec![Action::Fullscreen, Action::More];
        if width >= 246.0 * scale {
            secondary.splice(0..0, [Action::Volume, Action::Loop]);
        }
        if width >= 366.0 * scale {
            secondary.insert(2, Action::Rate);
        }
        if width >= 444.0 * scale {
            secondary.insert(0, Action::Open);
        }
        if update {
            secondary.insert(secondary.len() - 1, Action::Update);
        }
        let rate_extra = if secondary.contains(&Action::Rate) {
            10.0 * scale
        } else {
            0.0
        };
        let button_width = (38.0 * scale)
            .min((right - pad - rate_extra) / (primary.len() + secondary.len()) as f32);
        let row = |x, width| PxRect::new(x, 7.0 * scale, x + width, 45.0 * scale);
        let mut x = pad;
        let mut buttons = Vec::new();
        for action in primary {
            buttons.push((action, row(x, button_width)));
            x += button_width;
        }
        x = right - button_width * secondary.len() as f32 - rate_extra;
        for action in secondary {
            let width = button_width
                + if action == Action::Rate {
                    rate_extra
                } else {
                    0.0
                };
            buttons.push((action, row(x, width)));
            x += width;
        }
        Some(Self {
            offset,
            size,
            scale,
            buttons,
            seek,
            time,
            hint,
        })
    }

    pub fn hit(&self, point: POINT) -> Option<Hit> {
        if self.seek.contains(point) {
            return Some(Hit::Seek);
        }
        self.buttons
            .iter()
            .find_map(|(button, area)| area.contains(point).then_some(Hit::Button(*button)))
    }

    pub fn fraction(&self, x: i32) -> f32 {
        let rect = self.seek;
        ((x as f32 - rect.left) / (rect.right - rect.left).max(1.0)).clamp(0.0, 1.0)
    }

    pub fn contains(&self, point: POINT) -> bool {
        (0..self.size.cx).contains(&point.x) && (0..self.size.cy).contains(&point.y)
    }

    pub fn anchor(&self, action: Action) -> Option<POINT> {
        self.buttons.iter().find_map(|(button, rect)| {
            (*button == action).then_some(POINT {
                x: self.offset.x + rect.right.round() as i32,
                y: self.offset.y + rect.top.round() as i32,
            })
        })
    }

    pub fn focus(
        &self,
        previous: Option<Action>,
        reverse: bool,
        model: &Model<'_>,
    ) -> Option<Action> {
        let actions: Vec<_> = self
            .buttons
            .iter()
            .map(|(button, _)| *button)
            .filter(|button| model.enabled(*button))
            .collect();
        let len = actions.len();
        let index =
            previous.and_then(|previous| actions.iter().position(|button| *button == previous));
        let index = match (index, reverse) {
            (None, false) => 0,
            (None, true) => len.saturating_sub(1),
            (Some(index), false) => (index + 1) % len,
            (Some(index), true) => (index + len - 1) % len,
        };
        actions.get(index).copied()
    }
}

pub const RATES: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

pub struct Model<'a> {
    pub status: PlaybackStatus,
    pub preferences: Preferences,
    pub language: Language,
    pub fullscreen: bool,
    pub volume_open: bool,
    pub can_navigate: bool,
    pub update_busy: bool,
    pub hover: Option<Hit>,
    pub focus: Option<Action>,
    pub drag: Option<f32>,
    pub notice: Option<&'a str>,
}

impl Model<'_> {
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Previous | Action::Next => self.can_navigate,
            Action::Play => !matches!(self.status.phase, Phase::Opening),
            Action::Update => !self.update_busy,
            _ => true,
        }
    }

    pub fn label(&self, action: Action) -> &'static str {
        let labels = match action {
            Action::Previous => ("Vídeo anterior", "Previous video"),
            Action::Next => ("Vídeo siguiente", "Next video"),
            Action::Play => match self.status.phase {
                Phase::Playing => ("Pausar", "Pause"),
                Phase::Ended => ("Repetir", "Replay"),
                Phase::Failed | Phase::MissingEngine => ("Volver a intentar", "Retry"),
                _ => ("Reproducir", "Play"),
            },
            Action::Volume => ("Volumen", "Volume"),
            Action::Loop if self.preferences.looping => ("Desactivar bucle", "Turn off loop"),
            Action::Loop => ("Repetir en bucle", "Loop video"),
            Action::Rate => ("Velocidad", "Playback speed"),
            Action::Fullscreen if self.fullscreen => {
                ("Salir de pantalla completa", "Exit full screen")
            }
            Action::Fullscreen => ("Pantalla completa", "Full screen"),
            Action::Open => ("Abrir archivo", "Open file"),
            Action::Update => ("Descargar actualización", "Download update"),
            Action::More => ("Más opciones", "More options"),
        };
        self.translate(labels)
    }

    fn translate(&self, labels: (&'static str, &'static str)) -> &'static str {
        match self.language {
            Language::Spanish => labels.0,
            Language::English => labels.1,
        }
    }

    fn glyph(&self, action: Action) -> u16 {
        match action {
            Action::Previous => 0xe892,
            Action::Play if self.status.phase == Phase::Playing => 0xe769,
            Action::Play => 0xe768,
            Action::Next => 0xe893,
            Action::Volume => speaker_glyph(self.preferences),
            Action::Loop => 0xe8ee,
            Action::Fullscreen if self.fullscreen => 0xe73f,
            Action::Fullscreen => 0xe740,
            Action::Open => 0xe8e5,
            Action::Update => 0xe896,
            Action::More => 0xe712,
            Action::Rate => 0,
        }
    }

    fn message(&self) -> Option<&str> {
        self.notice.or_else(|| {
            let labels = match self.status.phase {
                Phase::Opening if self.status.dimensions.is_none() => {
                    ("Abriendo vídeo…", "Opening video…")
                }
                Phase::MissingEngine => (
                    "Instala KGen View o extrae el ZIP portátil para reproducir vídeos.",
                    "Install KGen View or extract the portable ZIP to play video.",
                ),
                Phase::Failed => (
                    "No se ha podido reproducir el vídeo.",
                    "Could not play this video.",
                ),
                _ => return None,
            };
            Some(self.translate(labels))
        })
    }
}

pub fn rate_label(rate: f32, language: Language) -> String {
    let value = rate.to_string();
    let value = if language == Language::Spanish {
        value.replace('.', ",")
    } else {
        value
    };
    format!("{value}×")
}

pub struct Painter {
    scale: f32,
    icons: IDWriteTextFormat,
    time: IDWriteTextFormat,
    remaining: IDWriteTextFormat,
    hint: IDWriteTextFormat,
    rate: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
}

fn speaker_glyph(preferences: Preferences) -> u16 {
    if preferences.muted || preferences.volume == 0 {
        0xe74f
    } else {
        0xe767
    }
}

fn text_format(
    factory: &IDWriteFactory,
    name: PCWSTR,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let format = factory.CreateTextFormat(
            name,
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?;
        format.SetTextAlignment(align)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(format)
    }
}

impl Painter {
    fn new(target: &ID2D1DCRenderTarget, scale: f32) -> Result<Self> {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = |name, size, align| text_format(&factory, name, size * scale, align);
            Ok(Self {
                scale,
                icons: format(w!("Segoe MDL2 Assets"), 16.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                time: format(w!("Consolas"), 12.0, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                remaining: format(w!("Consolas"), 12.0, DWRITE_TEXT_ALIGNMENT_TRAILING)?,
                hint: format(w!("Segoe UI"), 12.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                rate: format(w!("Segoe UI"), 12.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                brush: target.CreateSolidColorBrush(&rgb(0xf1f1f6), None)?,
            })
        }
    }

    pub fn draw(
        painter: &mut Option<Self>,
        target: &ID2D1DCRenderTarget,
        layout: &PanelLayout,
        model: &Model<'_>,
    ) -> Result<()> {
        if painter
            .as_ref()
            .is_none_or(|value| value.scale != layout.scale)
        {
            *painter = Some(Self::new(target, layout.scale)?);
        }
        let painter = painter.as_ref().unwrap();
        unsafe { painter.scene(target, layout, model) };
        Ok(())
    }

    unsafe fn scene(&self, target: &ID2D1DCRenderTarget, layout: &PanelLayout, model: &Model<'_>) {
        const DISABLED: D2D1_COLOR_F = rgb(0x8a7367);
        let scale = layout.scale;
        let panel = PxRect::new(
            0.5,
            0.5,
            layout.size.cx as f32 - 0.5,
            layout.size.cy as f32 - 0.5,
        );
        let radius = 13.0 * scale;
        let brush = &self.brush;
        unsafe {
            brush.SetColor(&D2D1_COLOR_F {
                a: 0.94,
                ..rgb(0x1b1410)
            });
            target.FillRoundedRectangle(&panel.as_rounded(radius), brush);
            brush.SetColor(&D2D1_COLOR_F { a: 0.85, ..TRACK });
            target.DrawRoundedRectangle(&panel.as_rounded(radius), brush, 1.0, None);
        }
        let draw_chars = |chars: &[u16], area: PxRect, format: &IDWriteTextFormat, color| unsafe {
            brush.SetColor(color);
            target.DrawText(
                chars,
                format,
                &area.as_d2d(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        };
        let label = |value: &str, area, format, color| {
            draw_chars(
                &value.encode_utf16().collect::<Vec<_>>(),
                area,
                format,
                color,
            )
        };
        let preview = model
            .drag
            .unwrap_or_else(|| progress(model.status).unwrap_or(0.0));
        if let Some(message) = model.message() {
            label(message, layout.time, &self.time, &INK);
        } else {
            let elapsed = if model.drag.is_some() {
                (model.status.duration_ms as f32 * preview) as i64
            } else {
                model.status.elapsed_ms
            };
            let elapsed = if model.status.duration_ms > 0 {
                elapsed.clamp(0, model.status.duration_ms)
            } else {
                elapsed.max(0)
            };
            label(&time_label(elapsed), layout.time, &self.time, &INK);
            if model.status.duration_ms > 0 {
                let remaining = time_label(model.status.duration_ms.saturating_sub(elapsed).max(0));
                label(
                    &format!("−{remaining}"),
                    layout.time,
                    &self.remaining,
                    &MUTED,
                );
            }
            let selected = match model.hover {
                Some(Hit::Button(button)) => Some(model.label(button)),
                _ => model.focus.map(|button| model.label(button)),
            };
            if let Some(text) = selected {
                label(text, layout.hint, &self.hint, &MUTED);
            }
        }
        let track = |rect: PxRect, fraction: f32, thumb: bool| unsafe {
            let y = (rect.top + rect.bottom) * 0.5;
            let thickness = 3.0 * scale;
            let line = PxRect::new(
                rect.left,
                y - thickness * 0.5,
                rect.right,
                y + thickness * 0.5,
            );
            let x = line.left + (line.right - line.left) * fraction.clamp(0.0, 1.0);
            brush.SetColor(&TRACK);
            target.FillRoundedRectangle(&line.as_rounded(thickness * 0.5), brush);
            if x > line.left {
                brush.SetColor(&ACCENT);
                target.FillRoundedRectangle(
                    &PxRect { right: x, ..line }.as_rounded(thickness * 0.5),
                    brush,
                );
            }
            if thumb {
                brush.SetColor(&ACCENT);
                let thumb = PxRect::new(
                    x - 2.0 * scale,
                    y - 7.0 * scale,
                    x + 2.0 * scale,
                    y + 7.0 * scale,
                );
                brush.SetColor(&HOT_ACCENT);
                target.FillRoundedRectangle(&thumb.as_rounded(2.0 * scale), brush);
            }
        };
        track(layout.seek, preview, model.status.seekable);
        for (button, rect) in &layout.buttons {
            let rect = rect.inset(2.0 * scale);
            let enabled = model.enabled(*button);
            let highlighted =
                model.hover == Some(Hit::Button(*button)) || model.focus == Some(*button);
            let accent = enabled && *button == Action::Play;
            let color = if !enabled {
                DISABLED
            } else if accent {
                rgb(0x241b18)
            } else if (*button == Action::Loop && model.preferences.looping)
                || (*button == Action::Volume && model.volume_open)
                || *button == Action::Update
            {
                ACCENT
            } else {
                INK
            };
            unsafe {
                if accent || highlighted {
                    brush.SetColor(&if accent {
                        if highlighted { HOT_ACCENT } else { ACCENT }
                    } else {
                        rgb(0x4b2d1e)
                    });
                    target.FillRoundedRectangle(&rect.as_rounded(8.0 * scale), brush);
                }
                if model.focus == Some(*button) {
                    brush.SetColor(&INK);
                    target.DrawRoundedRectangle(&rect.as_rounded(8.0 * scale), brush, 1.0, None);
                }
            }
            if *button == Action::Rate {
                label(
                    &rate_label(model.preferences.rate, model.language),
                    rect,
                    &self.rate,
                    &color,
                );
            } else {
                draw_chars(&[model.glyph(*button)], rect, &self.icons, &color);
            }
        }
    }
}

fn progress(status: PlaybackStatus) -> Option<f32> {
    (status.duration_ms > 0)
        .then(|| (status.elapsed_ms as f32 / status.duration_ms as f32).clamp(0.0, 1.0))
}

fn time_label(ms: i64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    if minutes < 60 {
        format!("{minutes}:{:02}", seconds % 60)
    } else {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    }
}
