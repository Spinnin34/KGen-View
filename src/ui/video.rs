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
    platform::playback::{Chapter, MediaDetails, Phase, PlaybackStatus, Preferences},
};

use super::{layout::PxRect, theme::rgb};

mod edge;
mod layer;
mod menu;
pub use edge::{EdgeLayout, EdgeZone, ProgressLine, VolumeHit, VolumeLayout, VolumePainter};
pub use layer::{
    Layer, LayerKind, WM_PANEL_INPUT, WM_VIDEO_MENU_INPUT, WM_VOLUME_INPUT,
    set_video_cursor_hidden, video_cursor_hidden,
};
pub use menu::{MenuCommand, MenuHit, MenuPage, MenuRow, VideoMenu};

const ACCENT: D2D1_COLOR_F = rgb(0xff8d42);
const HOT_ACCENT: D2D1_COLOR_F = rgb(0xffbb89);
const ACCENT_HALO: D2D1_COLOR_F = D2D1_COLOR_F { a: 0.19, ..ACCENT };
const INK: D2D1_COLOR_F = rgb(0xf4f4f4);
const MUTED: D2D1_COLOR_F = rgb(0xb4b4b8);
const TRACK: D2D1_COLOR_F = rgb(0x494950);
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
    Tracks,
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
    body: PxRect,
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
            .min(height as f32 / 108.0)
            .min((width as f32 / 380.0).max(0.74))
            .min(width as f32 / 138.0);
        let inset = horizontal_inset(width, scale);
        let size = SIZE {
            cx: width - 2 * inset,
            cy: (88.0 * scale).round() as i32,
        };
        let width = size.cx as f32;
        let offset = POINT {
            x: client.left + inset,
            y: client.bottom - size.cy - (10.0 * scale).round() as i32,
        };
        let body = PxRect::new(0.0, 22.0 * scale, width, size.cy as f32);
        let pad = 10.0 * scale;
        let right = width - pad;
        let seek = PxRect::new(16.0 * scale, body.top, width - 16.0 * scale, 47.0 * scale);
        let primary = [Action::Previous, Action::Play, Action::Next];
        let mut secondary = vec![Action::Fullscreen, Action::More];
        if width >= 237.0 * scale {
            secondary.splice(0..0, [Action::Volume, Action::Loop]);
        }
        if width >= 370.0 * scale {
            secondary.insert(1, Action::Rate);
        }
        if width >= 460.0 * scale {
            secondary.insert(1, Action::Tracks);
        }
        if width >= 735.0 * scale {
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
        let button_width = (33.0 * scale)
            .min((right - pad - rate_extra) / (primary.len() + secondary.len()) as f32);
        let primary_width = button_width * primary.len() as f32;
        let secondary_left = right - button_width * secondary.len() as f32 - rate_extra;
        let center = (width - primary_width) * 0.5;
        let centered =
            center >= pad + 132.0 * scale && center + primary_width + 6.0 * scale <= secondary_left;
        let primary_left = if centered { center } else { pad };
        let primary_right = primary_left + primary_width;

        let row =
            |left: f32, width: f32| PxRect::new(left, seek.bottom, left + width, 84.0 * scale);
        let time_left = if centered {
            16.0 * scale
        } else {
            primary_right + 9.0 * scale
        };
        let time_right = if centered {
            primary_left - 9.0 * scale
        } else {
            secondary_left - 4.0 * scale
        };
        let time = row(
            time_left,
            (time_right - time_left).clamp(0.0, 143.0 * scale),
        );
        let hint_left = time
            .right
            .max(if centered { primary_right } else { time.left });
        let caption = row(hint_left, (secondary_left - hint_left).max(0.0));
        let mut buttons: Vec<_> = primary
            .into_iter()
            .enumerate()
            .map(|(index, action)| {
                (
                    action,
                    row(primary_left + button_width * index as f32, button_width),
                )
            })
            .collect();
        let mut x = secondary_left;
        for action in secondary {
            let width = if action == Action::Rate {
                button_width + rate_extra
            } else {
                button_width
            };
            buttons.push((action, row(x, width)));
            x += width;
        }
        Some(Self {
            offset,
            size,
            scale,
            body,
            buttons,
            seek,
            time,
            caption,
        })
    }

    pub fn hit(&self, point: POINT) -> Option<Hit> {
        if (self.seek.top..self.seek.bottom).contains(&(point.y as f32)) && self.contains(point) {
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

    pub fn seek_fraction(&self, x: i32, duration_ms: i64, chapters: &[Chapter]) -> f32 {
        let fraction = self.fraction(x);
        self.chapter_near(fraction, duration_ms, chapters)
            .map_or(fraction, |chapter| {
                time_fraction(chapter.start_ms, duration_ms)
            })
    }

    fn chapter_near<'a>(
        &self,
        fraction: f32,
        duration_ms: i64,
        chapters: &'a [Chapter],
    ) -> Option<&'a Chapter> {
        if duration_ms <= 0 {
            return None;
        }
        let target = (duration_ms as f64 * f64::from(fraction)).round() as i64;
        let next = chapters.partition_point(|chapter| chapter.start_ms < target);
        let tolerance = 5.5 * self.scale / (self.seek.right - self.seek.left);
        [
            next.checked_sub(1).and_then(|index| chapters.get(index)),
            chapters.get(next),
        ]
        .into_iter()
        .flatten()
        .filter(|chapter| chapter.start_ms < duration_ms)
        .min_by_key(|chapter| chapter.start_ms.abs_diff(target))
        .filter(|chapter| {
            (time_fraction(chapter.start_ms, duration_ms) - fraction).abs() <= tolerance
        })
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
        self.body.contains(point)
    }

    pub fn body_top(&self) -> i32 {
        self.offset.y + self.body.top.round() as i32
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
    pub media: Option<&'a MediaDetails>,
}

impl Model<'_> {
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Previous | Action::Next => self.can_navigate,
            Action::Tracks => self.media.is_some_and(|media| {
                (self.status.seekable && !media.chapters.is_empty())
                    || media
                        .audio
                        .iter()
                        .chain(&media.subtitles)
                        .any(|track| track.id >= 0)
            }),
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
            Action::Tracks => ("Pistas y capítulos", "Tracks and chapters"),
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
            Action::Tracks => 0xed1f,
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
    caption: IDWriteTextFormat,
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
                caption: format(w!("Segoe UI"), 11.0, DWRITE_TEXT_ALIGNMENT_LEADING)?,
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
        surface(target, brush, layout.body, scale);
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
        let chapters = model
            .media
            .map_or(&[][..], |media| media.chapters.as_slice());
        if let Some(message) = model.message() {
            label(message, layout.seek, &self.label, &INK);
        } else {
            let clock = layout.time;
            let clock_width = clock.right - clock.left;
            let preview = model
                .drag
                .or(model.seek_hover)
                .filter(|_| model.status.seekable);
            if clock_width >= 30.0 * scale {
                let elapsed = time_label(timeline_elapsed(model.status, model.drag));
                let duration = time_label(model.status.duration_ms.max(0));
                let elapsed_width = elapsed.len() as f32 * 6.4 * scale;
                let duration_left = clock.left + elapsed_width + 10.0 * scale;
                label(&elapsed, clock, &self.time, &INK);
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
            let preview_x = preview.map(|fraction| seek.left + (seek.right - seek.left) * fraction);
            let active = preview.is_some();
            let half = if active { 2.4 } else { 1.85 } * scale;
            let rail = PxRect::new(seek.left, y - half, seek.right, y + half);
            unsafe {
                brush.SetColor(&TRACK);
                target.FillRoundedRectangle(&rail.as_rounded(half), brush);
                if x > rail.left {
                    let played = PxRect { right: x, ..rail };
                    brush.SetColor(&ACCENT);
                    target.FillRoundedRectangle(
                        &played.as_rounded(half.min((x - rail.left) * 0.5)),
                        brush,
                    );
                }
                brush.SetColor(&rgb(0xd1c7c2));
                let mut last_x = seek.left;
                for chapter in chapters.iter().filter(|chapter| {
                    chapter.start_ms > 0 && chapter.start_ms < model.status.duration_ms
                }) {
                    let marker = seek.left
                        + (seek.right - seek.left)
                            * time_fraction(chapter.start_ms, model.status.duration_ms);
                    if marker - last_x >= 3.5 * scale {
                        let tick = PxRect::new(
                            marker - 0.5 * scale,
                            y - 4.8 * scale,
                            marker + 0.5 * scale,
                            y + 4.8 * scale,
                        );
                        target.FillRectangle(&tick.as_d2d(), brush);
                        last_x = marker;
                    }
                }
                if let Some(cursor) = preview_x
                    && model.drag.is_none()
                    && (cursor - x).abs() > 6.0 * scale
                {
                    brush.SetColor(&HOT_ACCENT);
                    let marker = PxRect::new(
                        cursor - 0.7 * scale,
                        y - 5.8 * scale,
                        cursor + 0.7 * scale,
                        y + 5.8 * scale,
                    );
                    target.FillRoundedRectangle(&marker.as_rounded(0.7 * scale), brush);
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
            if let Some((fraction, cursor)) = preview.zip(preview_x) {
                let milliseconds = timeline_elapsed(model.status, Some(fraction));
                let timecode = format!("{}.{:03}", time_label(milliseconds), milliseconds % 1000);
                let chapter = layout
                    .chapter_near(fraction, model.status.duration_ms, chapters)
                    .map(|chapter| chapter.name.as_str())
                    .filter(|name| !name.is_empty());
                let gap = 8.0 * scale;
                let time_width = timecode.len() as f32 * 6.4 * scale + 2.0 * gap;
                let max_extra =
                    (layout.body.right - time_width - 2.0 * gap).clamp(0.0, 176.0 * scale);
                let chapter = chapter.filter(|_| max_extra >= 50.0 * scale);
                let extra = chapter.map_or(0.0, |name| {
                    ((name.chars().count() as f32 * 5.8 + 18.0) * scale).min(max_extra)
                });
                let width = time_width + extra;
                let left = (cursor - width * 0.5).clamp(0.0, (layout.body.right - width).max(0.0));
                let badge =
                    PxRect::new(left + 0.5, 0.5, left + width - 0.5, seek.top + 1.0 * scale);
                unsafe {
                    let rounded = badge.as_rounded(5.0 * scale);
                    brush.SetColor(&rgb(0x252429));
                    target.FillRoundedRectangle(&rounded, brush);
                    brush.SetColor(&HAIRLINE);
                    target.DrawRoundedRectangle(&rounded, brush, 1.0, None);
                }
                label(
                    &timecode,
                    PxRect {
                        left: left + gap,
                        right: left + time_width,
                        ..badge
                    },
                    &self.time,
                    &INK,
                );
                if let Some(chapter) = chapter {
                    let max_chars = ((extra - gap) / (6.1 * scale)).floor() as usize;
                    let mut text: String = chapter.chars().take(max_chars).collect();
                    if text.len() < chapter.len() {
                        text.pop();
                        text.push('…');
                    }
                    label(
                        &text,
                        PxRect {
                            left: left + time_width,
                            right: badge.right - gap,
                            ..badge
                        },
                        &self.caption,
                        &MUTED,
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
                || (*button == Action::Tracks && model.status.subtitle_track >= 0)
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
            match button {
                Action::Rate => label(
                    &rate_label(model.preferences.rate, model.language),
                    rect,
                    &self.label,
                    &color,
                ),
                _ => draw_chars(&[model.glyph(*button)], rect, &self.icons, &color),
            }
        }
    }
}

fn horizontal_inset(width: i32, scale: f32) -> i32 {
    (width as f32 * 0.022)
        .clamp(8.0 * scale, 24.0 * scale)
        .round() as i32
}

fn surface(target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush, bounds: PxRect, scale: f32) {
    let bounds = bounds.inset(0.5);
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
    (status.duration_ms > 0).then(|| time_fraction(status.elapsed_ms, status.duration_ms))
}

fn time_fraction(elapsed_ms: i64, duration_ms: i64) -> f32 {
    (elapsed_ms as f64 / duration_ms as f64).clamp(0.0, 1.0) as f32
}

pub fn time_label(ms: i64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    if minutes < 60 {
        format!("{minutes}:{:02}", seconds % 60)
    } else {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    }
}
