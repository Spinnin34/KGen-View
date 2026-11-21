use windows::{
    Win32::{
        Foundation::{POINT, RECT, SIZE},
        Graphics::{
            Direct2D::{
                Common::D2D1_COLOR_F, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
                ID2D1DCRenderTarget, ID2D1SolidColorBrush,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
                DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
                IDWriteTextFormat,
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
const HOT_ACCENT: D2D1_COLOR_F = rgb(0xffbb89);
const ACCENT_HALO: D2D1_COLOR_F = D2D1_COLOR_F { a: 0.19, ..ACCENT };
const INK: D2D1_COLOR_F = rgb(0xf4f4f4);
const MUTED: D2D1_COLOR_F = rgb(0xb4b4b8);
const TRACK: D2D1_COLOR_F = rgb(0x4e4e54);
const HAIRLINE: D2D1_COLOR_F = rgb(0x3d3d42);
const PANEL: D2D1_COLOR_F = D2D1_COLOR_F {
    a: 0.94,
    ..rgb(0x19191d)
};
const HOVER: D2D1_COLOR_F = rgb(0x36363d);

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
    caption: PxRect,
}

impl PanelLayout {
    pub fn new(client: RECT, dpi_scale: f32, update: bool) -> Option<Self> {
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width < 100 || height < 60 {
            return None;
        }
        let scale = dpi_scale
            .min(height as f32 / 78.0)
            .min((width as f32 / 360.0).max(0.72))
            .min(width as f32 / 132.0);
        let inset = horizontal_inset(width, scale);
        let size = SIZE {
            cx: width - 2 * inset,
            cy: (58.0 * scale).round() as i32,
        };
        let width = size.cx as f32;
        let offset = POINT {
            x: client.left + inset,
            y: client.bottom - size.cy - (10.0 * scale).round() as i32,
        };
        let pad = 8.0 * scale;
        let right = width - pad;
        let seek = PxRect::new(
            14.0 * scale,
            38.0 * scale,
            width - 14.0 * scale,
            size.cy as f32,
        );
        let primary = [Action::Previous, Action::Play, Action::Next];
        let mut secondary = vec![Action::Fullscreen, Action::More];
        if width >= 230.0 * scale {
            secondary.splice(0..0, [Action::Volume, Action::Loop]);
        }
        if width >= 360.0 * scale {
            secondary.insert(2, Action::Rate);
        }
        if width >= 452.0 * scale {
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
        let button_width = (32.0 * scale)
            .min((right - pad - rate_extra) / (primary.len() + secondary.len()) as f32);
        let row = |x, width| PxRect::new(x, 5.0 * scale, x + width, 38.0 * scale);
        let mut x = pad;
        let mut buttons = Vec::new();
        for action in primary {
            buttons.push((action, row(x, button_width)));
            x += button_width;
        }
        let left = x;
        x = right - button_width * secondary.len() as f32 - rate_extra;
        let free = (x - left).max(0.0);
        let time_width = free.min(147.0 * scale);
        let time = row(left + 11.0 * scale, (time_width - 11.0 * scale).max(0.0));
        let caption = row(left + time_width, free - time_width);
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
            caption,
        })
    }

    pub fn hit(&self, point: POINT) -> Option<Hit> {
        if point.y as f32 >= self.seek.top && self.contains(point) {
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

    pub fn timeline_key(&self, status: PlaybackStatus, drag: Option<f32>) -> (i32, i64) {
        let elapsed = timeline_elapsed(status, drag);
        (
            (self.playhead_x(status, drag) * 4.0).round() as i32,
            elapsed / 1000,
        )
    }

    fn playhead_x(&self, status: PlaybackStatus, drag: Option<f32>) -> f32 {
        let fraction = drag.or_else(|| progress(status)).unwrap_or(0.0);
        self.seek.left + (self.seek.right - self.seek.left) * fraction
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
    pub seek_hover: Option<f32>,
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
    label: IDWriteTextFormat,
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
                icons: format(w!("Segoe MDL2 Assets"), 15.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                time: format(w!("Consolas"), 11.5, DWRITE_TEXT_ALIGNMENT_LEADING)?,
                label: format(w!("Segoe UI"), 11.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
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
        const DISABLED: D2D1_COLOR_F = rgb(0x77777b);
        let scale = layout.scale;
        let brush = &self.brush;
        surface(target, brush, layout.size, scale);
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
        if let Some(message) = model.message() {
            label(message, layout.seek, &self.label, &INK);
        } else {
            let clock = layout.time;
            let clock_width = clock.right - clock.left;
            let preview = model.drag.or(model.seek_hover);
            if clock_width >= 30.0 * scale {
                let elapsed = time_label(timeline_elapsed(model.status, preview));
                let duration = time_label(model.status.duration_ms.max(0));
                let elapsed_width = elapsed.len() as f32 * 6.4 * scale;
                let duration_left = clock.left + elapsed_width + 10.0 * scale;
                let center_y = (clock.top + clock.bottom) * 0.5;
                let split = PxRect::new(
                    clock.left - 6.0 * scale,
                    center_y - 7.0 * scale,
                    clock.left - 5.0 * scale,
                    center_y + 7.0 * scale,
                );
                unsafe {
                    brush.SetColor(&HAIRLINE);
                    target.FillRectangle(&split.as_d2d(), brush);
                }
                label(
                    &elapsed,
                    clock,
                    &self.time,
                    &if preview.is_some() { HOT_ACCENT } else { INK },
                );
                if model.status.duration_ms > 0
                    && clock.right - duration_left >= (duration.len() + 2) as f32 * 6.4 * scale
                {
                    label(
                        &format!("/ {duration}"),
                        PxRect {
                            left: duration_left,
                            ..clock
                        },
                        &self.time,
                        &MUTED,
                    );
                }
            }
            let caption = match model.hover {
                Some(Hit::Button(button)) => Some(model.label(button)),
                _ => model.focus.map(|button| model.label(button)),
            };
            if let Some(caption) = caption.filter(|text| {
                layout.caption.right - layout.caption.left
                    >= (text.chars().count() as f32 * 5.4 + 6.0) * scale
            }) {
                label(caption, layout.caption, &self.label, &MUTED);
            }
            let seek = layout.seek;
            let y = (seek.top + seek.bottom) * 0.5;
            let x = layout.playhead_x(model.status, model.drag);
            let active = model.status.seekable && preview.is_some();
            let half = if active { 1.75 } else { 1.25 } * scale;
            let rail = PxRect::new(seek.left, y - half, seek.right, y + half);
            unsafe {
                brush.SetColor(&TRACK);
                target.FillRoundedRectangle(&rail.as_rounded(half), brush);
                let cursor = model
                    .seek_hover
                    .filter(|_| model.drag.is_none())
                    .map(|fraction| rail.left + (rail.right - rail.left) * fraction);
                if x > rail.left {
                    let played = PxRect { right: x, ..rail };
                    brush.SetColor(&ACCENT);
                    target.FillRoundedRectangle(
                        &played.as_rounded(half.min((x - rail.left) * 0.5)),
                        brush,
                    );
                }
                if let Some(cursor) = cursor
                    && (cursor - x).abs() > 7.0 * scale
                {
                    let span = PxRect {
                        left: cursor.min(x),
                        right: cursor.max(x),
                        ..rail
                    };
                    brush.SetColor(&if cursor > x {
                        rgb(0x855b44)
                    } else {
                        rgb(0xca6e3c)
                    });
                    target.FillRoundedRectangle(&span.as_rounded(half), brush);
                    let marker = PxRect::new(
                        cursor - 0.65 * scale,
                        y - 4.0 * scale,
                        cursor + 0.65 * scale,
                        y + 4.0 * scale,
                    );
                    brush.SetColor(&HOT_ACCENT);
                    target.FillRoundedRectangle(&marker.as_rounded(0.65 * scale), brush);
                }
                if model.status.seekable {
                    draw_thumb(
                        target,
                        brush,
                        x,
                        y,
                        scale,
                        active,
                        model.drag.is_some(),
                        false,
                    );
                }
            }
        }
        for (button, rect) in &layout.buttons {
            let rect = rect.inset(scale);
            let enabled = model.enabled(*button);
            let highlighted =
                model.hover == Some(Hit::Button(*button)) || model.focus == Some(*button);
            let color = if !enabled {
                DISABLED
            } else if *button == Action::Play && highlighted {
                HOT_ACCENT
            } else if *button == Action::Play
                || (*button == Action::Loop && model.preferences.looping)
                || (*button == Action::Volume && model.volume_open)
                || *button == Action::Update
            {
                ACCENT
            } else {
                INK
            };
            unsafe {
                let size = (rect.right - rect.left).min(rect.bottom - rect.top) * 0.5;
                let x = (rect.left + rect.right) * 0.5;
                let y = (rect.top + rect.bottom) * 0.5;
                let square = PxRect::new(x - size, y - size, x + size, y + size);
                let button_shape = square.as_rounded((8.0 * scale).min(size));
                if enabled && (*button == Action::Play || highlighted) {
                    let background = match (*button == Action::Play, highlighted) {
                        (true, true) => rgb(0x4a342a),
                        (true, false) => rgb(0x372926),
                        (false, _) => HOVER,
                    };
                    brush.SetColor(&background);
                    target.FillRoundedRectangle(&button_shape, brush);
                }
                if model.focus == Some(*button) {
                    brush.SetColor(&INK);
                    target.DrawRoundedRectangle(&button_shape, brush, 1.0, None);
                }
            }
            if *button == Action::Rate {
                label(
                    &rate_label(model.preferences.rate, model.language),
                    rect,
                    &self.label,
                    &color,
                );
            } else {
                draw_chars(&[model.glyph(*button)], rect, &self.icons, &color);
            }
        }
    }
}

fn horizontal_inset(width: i32, scale: f32) -> i32 {
    (width as f32 * 0.022)
        .clamp(8.0 * scale, 24.0 * scale)
        .round() as i32
}

fn surface(target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush, size: SIZE, scale: f32) {
    let bounds = PxRect::new(0.5, 0.5, size.cx as f32 - 0.5, size.cy as f32 - 0.5);
    let rounded = bounds.as_rounded(7.0 * scale);
    unsafe {
        brush.SetColor(&PANEL);
        target.FillRoundedRectangle(&rounded, brush);
        brush.SetColor(&D2D1_COLOR_F {
            a: 0.86,
            ..HAIRLINE
        });
        target.DrawRoundedRectangle(&rounded, brush, 1.0, None);
    }
}

fn draw_thumb(
    target: &ID2D1DCRenderTarget,
    brush: &ID2D1SolidColorBrush,
    x: f32,
    y: f32,
    scale: f32,
    active: bool,
    dragging: bool,
    muted: bool,
) {
    let radius = if active { 4.4 } else { 3.1 } * scale;
    unsafe {
        if active && !muted {
            brush.SetColor(&ACCENT_HALO);
            target.FillEllipse(&dot(x, y, radius + 3.0 * scale), brush);
        }
        brush.SetColor(&rgb(0x211d1c));
        target.FillEllipse(&dot(x, y, radius + 0.9 * scale), brush);
        let color = if muted {
            MUTED
        } else if dragging {
            HOT_ACCENT
        } else {
            ACCENT
        };
        brush.SetColor(&color);
        target.FillEllipse(&dot(x, y, radius), brush);
    }
}

fn dot(x: f32, y: f32, radius: f32) -> D2D1_ELLIPSE {
    let mut ellipse = D2D1_ELLIPSE {
        radiusX: radius,
        radiusY: radius,
        ..Default::default()
    };
    ellipse.point.X = x;
    ellipse.point.Y = y;
    ellipse
}

fn timeline_elapsed(status: PlaybackStatus, drag: Option<f32>) -> i64 {
    let elapsed = drag.map_or(status.elapsed_ms, |fraction| {
        (status.duration_ms as f64 * f64::from(fraction)).round() as i64
    });
    elapsed.clamp(
        0,
        if status.duration_ms > 0 {
            status.duration_ms
        } else {
            i64::MAX
        },
    )
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
