use windows::Win32::{
    Foundation::{COLORREF, POINT, RECT},
    Graphics::Gdi::{
        CreateSolidBrush, DEFAULT_GUI_FONT, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE,
        DT_VCENTER, DeleteObject, DrawTextW, FillRect, GetStockObject, HBRUSH, HDC, HGDIOBJ,
        SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    },
};

use crate::{
    i18n::Language,
    platform::playback::{Phase, PlaybackStatus},
};

use super::layout::{ChromeLayout, PxRect};

pub struct Bar {
    pub bounds: PxRect,
    play: PxRect,
    seek: PxRect,
    time: PxRect,
    mute: PxRect,
    volume: PxRect,
    scale: f32,
}

pub enum Click {
    Pause,
    Seek(f64),
    Mute,
    Volume(i32),
}

impl Bar {
    pub fn new(layout: &ChromeLayout) -> Self {
        let area = layout.image;
        let scale = layout.scale;
        let bounds = PxRect::new(
            area.left,
            (area.bottom - 42.0 * scale).max(area.top),
            area.right,
            area.bottom,
        );
        let row = |left, right| {
            PxRect::new(
                left,
                bounds.top + 6.0 * scale,
                right,
                bounds.bottom - 6.0 * scale,
            )
        };
        let width = (area.right - area.left) / scale;
        let x = |dip| area.left + dip * scale;
        let right = |dip| area.right - dip * scale;
        let show_volume = width >= 360.0;
        let show_time = width >= 465.0;
        let mute_right = if show_volume { right(80.0) } else { right(8.0) };
        let mute_left = mute_right - 82.0 * scale;
        let time_left = mute_left - if show_time { 122.0 * scale } else { 0.0 };
        Self {
            bounds,
            play: row(x(8.0), x(95.0).min(mute_left - 4.0 * scale)),
            seek: row(x(107.0), time_left - 9.0 * scale),
            time: if show_time {
                row(time_left, mute_left - 4.0 * scale)
            } else {
                PxRect::default()
            },
            mute: row(mute_left, mute_right),
            volume: if show_volume {
                row(right(70.0), right(10.0))
            } else {
                PxRect::default()
            },
            scale,
        }
    }

    pub fn hit(&self, point: POINT) -> Option<Click> {
        let fraction = |rect: PxRect| {
            f64::from((point.x as f32 - rect.left) / (rect.right - rect.left)).clamp(0.0, 1.0)
        };
        if self.play.contains(point) {
            Some(Click::Pause)
        } else if self.seek.contains(point) {
            Some(Click::Seek(fraction(self.seek)))
        } else if self.mute.contains(point) {
            Some(Click::Mute)
        } else if self.volume.contains(point) {
            Some(Click::Volume((fraction(self.volume) * 100.0).round() as i32))
        } else {
            None
        }
    }
}

pub fn message(language: Language, phase: Phase) -> &'static str {
    match (language, phase) {
        (Language::Spanish, Phase::Opening) => "Abriendo vídeo…",
        (Language::English, Phase::Opening) => "Opening video…",
        (Language::Spanish, Phase::MissingEngine) => {
            "Motor ausente. Usa el instalador o el ZIP portátil."
        }
        (Language::English, Phase::MissingEngine) => {
            "Playback engine missing. Use the installer or portable ZIP."
        }
        (Language::Spanish, Phase::Failed) => "No se ha podido reproducir el vídeo.",
        (Language::English, Phase::Failed) => "Could not play this video.",
        _ => "",
    }
}

pub unsafe fn draw(
    hdc: HDC,
    bar: &Bar,
    status: PlaybackStatus,
    volume: i32,
    muted: bool,
    language: Language,
    dark: bool,
) {
    let [background, raised, accent, text_color] = if dark {
        [0x00181818, 0x00343434, 0x002A8AFF, 0x00FFFFFF]
    } else {
        [0x00FBFAFA, 0x00E6E1DC, 0x001667EA, 0x00202020]
    };
    let [background, raised, accent] =
        [background, raised, accent].map(|color| unsafe { CreateSolidBrush(COLORREF(color)) });
    let old_font = unsafe { SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT)) };
    let old_mode = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let old_color = unsafe { SetTextColor(hdc, COLORREF(text_color)) };
    let fill = |area, brush| unsafe { fill(hdc, area, brush) };
    let label = |area, text| unsafe { label(hdc, area, text) };
    fill(bar.bounds, background);
    let message = message(language, status.phase);
    if !message.is_empty() {
        label(bar.bounds.inset(6.0 * bar.scale), message);
    } else {
        let play = match (language, status.phase) {
            (Language::Spanish, Phase::Playing) => "Pausa",
            (Language::English, Phase::Playing) => "Pause",
            (Language::Spanish, Phase::Ended) => "Repetir",
            (Language::English, Phase::Ended) => "Replay",
            (Language::Spanish, _) => "Reproducir",
            (Language::English, _) => "Play",
        };
        let audio = match (language, muted) {
            (Language::Spanish, true) => "Dar sonido",
            (Language::Spanish, false) => "Silenciar",
            (Language::English, true) => "Unmute",
            (Language::English, false) => "Mute",
        };
        fill(bar.play, raised);
        label(bar.play, play);
        fill(bar.mute, raised);
        label(bar.mute, audio);
        let track = |area: PxRect, fraction: f32| {
            let center = (area.top + area.bottom) * 0.5;
            let height = 2.0 * bar.scale;
            let area = PxRect::new(area.left, center - height, area.right, center + height);
            fill(area, raised);
            fill(
                PxRect {
                    right: area.left + (area.right - area.left) * fraction.clamp(0.0, 1.0),
                    ..area
                },
                accent,
            );
        };
        if status.duration_ms > 0 {
            track(
                bar.seek,
                status.elapsed_ms as f32 / status.duration_ms as f32,
            );
        }
        track(bar.volume, if muted { 0.0 } else { volume as f32 / 100.0 });
        if bar.time.right > bar.time.left {
            let elapsed = time_label(status.elapsed_ms);
            let duration = time_label(status.duration_ms);
            label(bar.time, &format!("{elapsed} / {duration}"));
        }
    }
    unsafe {
        SelectObject(hdc, old_font);
        SetTextColor(hdc, old_color);
        SetBkMode(
            hdc,
            windows::Win32::Graphics::Gdi::BACKGROUND_MODE(old_mode as u32),
        );
        for brush in [background, raised, accent] {
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }
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

fn rect(area: PxRect) -> Option<RECT> {
    (area.right > area.left && area.bottom > area.top).then_some(RECT {
        left: area.left.round() as i32,
        top: area.top.round() as i32,
        right: area.right.round() as i32,
        bottom: area.bottom.round() as i32,
    })
}

unsafe fn fill(hdc: HDC, area: PxRect, brush: HBRUSH) {
    if let Some(rect) = rect(area) {
        unsafe { FillRect(hdc, &rect, brush) };
    }
}

unsafe fn label(hdc: HDC, area: PxRect, value: &str) {
    if let Some(mut rect) = rect(area) {
        let mut text: Vec<u16> = value.encode_utf16().collect();
        unsafe {
            DrawTextW(
                hdc,
                &mut text,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        }
    }
}
