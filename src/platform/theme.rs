use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND},
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        },
    },
    core::BOOL,
};

const DARK_CHROME: COLORREF = COLORREF(0x00181818);
const DARK_BORDER: COLORREF = COLORREF(0x00353535);
const LIGHT_TEXT: COLORREF = COLORREF(0x00f2f2f2);

/// Applies documented DWM attributes only. All client chrome is custom drawn, so
/// it never falls back to bright legacy controls when Windows is in dark mode.
pub unsafe fn apply_window_theme(hwnd: HWND, dark: bool) {
    let enabled = BOOL::from(dark);
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&enabled as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        )
    };

    let caption = if dark {
        DARK_CHROME
    } else {
        COLORREF(0x00f7f7f7)
    };
    let border = if dark {
        DARK_BORDER
    } else {
        COLORREF(0x00d8d8d8)
    };
    let text = if dark {
        LIGHT_TEXT
    } else {
        COLORREF(0x00141414)
    };
    for (attribute, value) in [
        (DWMWA_CAPTION_COLOR, caption),
        (DWMWA_BORDER_COLOR, border),
        (DWMWA_TEXT_COLOR, text),
    ] {
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                (&value as *const COLORREF).cast(),
                size_of::<COLORREF>() as u32,
            )
        };
    }
}
