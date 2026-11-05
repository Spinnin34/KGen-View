use windows::Win32::{
    Foundation::{COLORREF, RECT},
    Graphics::Gdi::{
        CreateSolidBrush, DEFAULT_GUI_FONT, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER,
        DeleteObject, DrawTextW, FillRect, FrameRect, GetStockObject, HDC, HGDIOBJ, SelectObject,
        SetBkMode, SetTextColor, TRANSPARENT,
    },
};

use super::layout::ChromeLayout;

#[derive(Clone, Debug, Default)]
pub struct StatusModel {
    pub primary: String,
    pub position: Option<String>,
    pub zoom: Option<String>,
    pub size: Option<String>,
    pub date: Option<String>,
    pub message: Option<String>,
}

impl StatusModel {
    pub fn message(value: impl Into<String>) -> Self {
        Self {
            primary: value.into(),
            ..Self::default()
        }
    }
}

pub unsafe fn draw(hdc: HDC, layout: &ChromeLayout, model: &StatusModel, dark: bool) {
    if layout.status.bottom <= layout.status.top {
        return;
    }
    let status = layout.status;
    let pad = 2.0 * layout.scale;

    let cell_bg_color = if dark {
        COLORREF(0x001A1A1A)
    } else {
        COLORREF(0x00ECECEC)
    };
    let border_color = if dark {
        COLORREF(0x003A3A3A)
    } else {
        COLORREF(0x00B0B0B0)
    };
    let text_color = if dark {
        COLORREF(0x00FFFFFF) // Pure white text as requested
    } else {
        COLORREF(0x00202020)
    };
    let muted_text = if dark {
        COLORREF(0x00D0D0D0)
    } else {
        COLORREF(0x00404040)
    };

    let cell_bg = unsafe { CreateSolidBrush(cell_bg_color) };
    let border_brush = unsafe { CreateSolidBrush(border_color) };
    let old_font = unsafe { SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT)) };
    unsafe { SetBkMode(hdc, TRANSPARENT) };

    let segments = [
        (!model.primary.is_empty()).then_some((model.primary.as_str(), 145.0, text_color)),
        model.position.as_deref().map(|val| (val, 65.0, text_color)),
        model.zoom.as_deref().map(|val| (val, 55.0, text_color)),
        model.size.as_deref().map(|val| (val, 140.0, text_color)),
        model.date.as_deref().map(|val| (val, 145.0, text_color)),
        model.message.as_deref().map(|val| (val, 120.0, muted_text)),
    ];

    let mut x = status.left + pad;
    let cell_top = (status.top + 1.0 * layout.scale).round() as i32;
    let cell_bottom = (status.bottom - 1.0 * layout.scale).round() as i32;

    for item in segments.iter().flatten() {
        let (value, width_dip, color) = *item;
        let cell_w = (width_dip * layout.scale).round();
        if x + cell_w > status.right - pad {
            break;
        }

        let cell_rect = RECT {
            left: x.round() as i32,
            top: cell_top,
            right: (x + cell_w).round() as i32,
            bottom: cell_bottom,
        };

        unsafe {
            FillRect(hdc, &cell_rect, cell_bg);
            FrameRect(hdc, &cell_rect, border_brush);
            SetTextColor(hdc, color);
        }

        let mut text_rect = RECT {
            left: cell_rect.left + (4.0 * layout.scale).round() as i32,
            top: cell_rect.top,
            right: cell_rect.right - (4.0 * layout.scale).round() as i32,
            bottom: cell_rect.bottom,
        };

        let mut utf16: Vec<u16> = value.encode_utf16().collect();
        unsafe {
            DrawTextW(
                hdc,
                &mut utf16,
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }

        x += cell_w + 2.0 * layout.scale;
    }

    unsafe {
        SelectObject(hdc, old_font);
        let _ = DeleteObject(HGDIOBJ(cell_bg.0));
        let _ = DeleteObject(HGDIOBJ(border_brush.0));
    }
}
