use super::*;
use windows::Win32::UI::{
    Controls::WM_MOUSELEAVE,
    Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};

#[derive(Clone, Copy, Default)]
pub(super) struct Controls {
    pub formats: HWND,
    pub convert: HWND,
    pub status: HWND,
    pub close: HWND,
    source_label: HWND,
    pub source_name: HWND,
    format_label: HWND,
}

pub(super) struct Fonts {
    title: HFONT,
    text: HFONT,
    detail: HFONT,
    icon: HFONT,
}

impl Fonts {
    pub fn new(layout: Layout) -> Self {
        Self {
            title: font(layout.px(15), 600, w!("Segoe UI")),
            text: font(layout.px(14), 400, w!("Segoe UI")),
            detail: font(layout.px(12), 400, w!("Segoe UI")),
            icon: font(layout.px(12), 400, w!("Segoe MDL2 Assets")),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for font in [self.title, self.text, self.detail, self.icon] {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
        }
    }
}

fn font(size: i32, weight: i32, face: PCWSTR) -> HFONT {
    unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            face,
        )
    }
}

pub(super) fn create_controls(hwnd: HWND, state: &DialogState) -> windows::core::Result<Controls> {
    let create = |class, label: &str, id, style| {
        let label = wide(label);
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(label.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                0,
                0,
                Some(hwnd),
                (id != 0).then_some(HMENU(id as *mut _)),
                None,
                None,
            )
        }
    };
    const SS_NOPREFIX: WINDOW_STYLE = WINDOW_STYLE(0x80);
    const SS_CENTERIMAGE: WINDOW_STYLE = WINDOW_STYLE(0x200);
    const SS_ENDELLIPSIS: WINDOW_STYLE = WINDOW_STYLE(0x4000);
    let source_label = create(
        w!("STATIC"),
        state.label("Imagen abierta", "Open image"),
        0,
        SS_NOPREFIX,
    )?;
    let file_name = state
        .snapshot
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let source_name = create(
        w!("STATIC"),
        &file_name,
        0,
        SS_NOPREFIX | SS_CENTERIMAGE | SS_ENDELLIPSIS,
    )?;
    let format_label = create(
        w!("STATIC"),
        state.label("Formato de salida", "Output format"),
        0,
        SS_NOPREFIX,
    )?;
    let formats = create(
        w!("LISTBOX"),
        "",
        ID_FORMAT,
        WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(
                LBS_NOTIFY as u32
                    | LBS_HASSTRINGS as u32
                    | LBS_OWNERDRAWFIXED as u32
                    | LBS_NOINTEGRALHEIGHT as u32,
            ),
    )?;
    let instructions = if state.snapshot.frame_count() > 1 {
        state.label(
            "Se exportará el fotograma visible.",
            "The visible frame will be exported.",
        )
    } else {
        state.label(
            "Guarda una copia de la imagen.",
            "Save a copy of the image.",
        )
    };
    let status = create(w!("STATIC"), instructions, 0, SS_NOPREFIX)?;
    let button = WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32);
    let convert = create(
        w!("BUTTON"),
        state.label("Convertir", "Convert"),
        ID_CONVERT,
        button,
    )?;
    let close = create(
        w!("BUTTON"),
        state.label("Cerrar", "Close"),
        ID_CLOSE,
        button,
    )?;
    unsafe {
        let theme = if state.dark {
            w!("DarkMode_Explorer")
        } else {
            w!("Explorer")
        };
        let _ = SetWindowTheme(formats, theme, None);
        for format in &state.formats {
            let label = wide(&format.filter_label());
            let result = SendMessageW(
                formats,
                LB_ADDSTRING,
                None,
                Some(LPARAM(label.as_ptr() as isize)),
            );
            if result.0 < 0 {
                return Err(windows::core::Error::from_thread());
            }
        }
        for control in [formats, convert, close] {
            let _ = SetWindowSubclass(control, Some(track_hover), 1, 0);
        }
    }
    Ok(Controls {
        formats,
        convert,
        status,
        close,
        source_label,
        source_name,
        format_label,
    })
}

pub(super) fn position_controls(hwnd: HWND, state: &DialogState) {
    let controls = state.controls.get();
    let layout = state.layout.get();
    let fonts = state.fonts.borrow();
    let bottom = layout.list_bottom();
    let position = |control, rect: RECT| unsafe {
        let _ = SetWindowPos(
            control,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    };
    let label = |control, font: HFONT, top, right, bottom| {
        unsafe {
            SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        }
        position(control, layout.rect(16, top, right, bottom));
    };
    label(controls.source_label, fonts.detail, 53, WIDTH - 16, 70);
    label(controls.source_name, fonts.text, 71, WIDTH - 16, 91);
    label(controls.format_label, fonts.detail, 99, WIDTH - 16, 117);
    let row_height = layout.px(ROW_HEIGHT);
    let mut list = layout.rect(8, LIST_TOP, WIDTH - 8, bottom);
    list.bottom = list.top + row_height * layout.visible_rows;
    unsafe {
        SendMessageW(
            controls.formats,
            LB_SETITEMHEIGHT,
            None,
            Some(LPARAM(row_height as isize)),
        );
        SendMessageW(
            controls.formats,
            WM_SETFONT,
            Some(WPARAM(fonts.text.0 as usize)),
            None,
        );
    }
    position(controls.formats, list);
    label(
        controls.status,
        fonts.detail,
        bottom + 23,
        WIDTH - 137,
        bottom + 56,
    );
    position(
        controls.convert,
        layout.rect(WIDTH - 123, bottom + 20, WIDTH - 14, bottom + 54),
    );
    position(controls.close, layout.rect(WIDTH - 42, 7, WIDTH - 8, 36));
    let diameter = layout.px(CORNER_RADIUS * 2);
    unsafe {
        let region = CreateRoundRectRgn(0, 0, layout.width(), layout.height(), diameter, diameter);
        if !region.is_invalid() && SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

unsafe extern "system" fn track_hover(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match message {
            WM_MOUSEMOVE => {
                let mut event = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut event);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_MOUSELEAVE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(track_hover), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

pub(super) fn paint(hwnd: HWND, state: &DialogState) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let layout = state.layout.get();
    let palette = &state.palette;
    let fonts = state.fonts.borrow();
    let bounds = RECT {
        left: 0,
        top: 0,
        right: layout.width(),
        bottom: layout.height(),
    };
    fill(hdc, &bounds, palette.surface);
    round(
        hdc,
        bounds,
        palette.surface,
        palette.border,
        layout.px(CORNER_RADIUS * 2),
    );
    for top in [41, layout.list_bottom() + 8] {
        let mut divider = layout.rect(13, top, WIDTH - 13, top + 1);
        divider.bottom = divider.top + 1;
        fill(hdc, &divider, palette.border);
    }
    text(
        hdc,
        state.title(),
        layout.rect(16, 4, WIDTH - 44, 40),
        fonts.title,
        palette.text,
        DT_LEFT,
    );
    unsafe {
        let _ = EndPaint(hwnd, &paint);
    }
}

pub(super) fn draw_item(item: &DRAWITEMSTRUCT, state: &DialogState) {
    let hdc = item.hDC;
    let layout = state.layout.get();
    let palette = &state.palette;
    let fonts = state.fonts.borrow();
    let hovered = is_hovered(item);
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
    let rect = item.rcItem;
    fill(hdc, &rect, palette.surface);

    if item.CtlID as usize == ID_FORMAT {
        let Some(format) = state.formats.get(item.itemID as usize) else {
            return;
        };
        let row = RECT {
            right: rect.right - layout.px(2),
            bottom: rect.bottom - layout.px(2),
            ..rect
        };
        let background = if selected {
            palette.selected
        } else if hovered && !disabled {
            palette.hover
        } else {
            palette.surface
        };
        let border = if focused && !disabled {
            palette.accent
        } else {
            background
        };
        round(hdc, row, background, border, layout.px(10));
        let label = format.name.strip_suffix(" Encoder").unwrap_or(&format.name);
        let extension = format
            .extensions
            .iter()
            .map(|ext| format!(".{ext}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (label_right, detail_right) = (row.right - layout.px(185), row.right - layout.px(35));
        let label_rect = RECT {
            left: row.left + layout.px(8),
            right: label_right,
            ..row
        };
        let detail_rect = RECT {
            left: label_right + layout.px(6),
            right: detail_right,
            ..row
        };
        let color = if disabled {
            palette.muted
        } else if selected {
            palette.selected_text
        } else {
            palette.text
        };
        text(hdc, label, label_rect, fonts.text, color, DT_LEFT);
        text(
            hdc,
            &extension,
            detail_rect,
            fonts.detail,
            palette.muted,
            DT_RIGHT,
        );
        if selected {
            let check = RECT {
                left: detail_right + layout.px(4),
                right: row.right - layout.px(5),
                ..row
            };
            text(
                hdc,
                "\u{e73e}",
                check,
                fonts.icon,
                if disabled {
                    palette.muted
                } else {
                    palette.accent
                },
                DT_CENTER,
            );
        }
        return;
    }

    let convert = item.CtlID as usize == ID_CONVERT;
    let (background, color) = if disabled {
        (palette.hover, palette.muted)
    } else if convert && (hovered || selected) {
        (
            palette.selected_text,
            if state.dark { 0x20120b } else { 0xffffff },
        )
    } else if convert {
        (palette.accent, if state.dark { 0x20120b } else { 0xffffff })
    } else if hovered || selected {
        (palette.hover, palette.text)
    } else {
        (palette.surface, palette.muted)
    };
    let border = if focused { palette.text } else { background };
    round(hdc, rect, background, border, layout.px(10));
    let (label, font) = if convert {
        (state.label("Convertir", "Convert"), fonts.title)
    } else {
        ("\u{e711}", fonts.icon)
    };
    text(hdc, label, rect, font, color, DT_CENTER);
}

fn is_hovered(item: &DRAWITEMSTRUCT) -> bool {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point).is_ok()
            && WindowFromPoint(point) == item.hwndItem
            && ScreenToClient(item.hwndItem, &mut point).as_bool()
            && (item.rcItem.left..item.rcItem.right).contains(&point.x)
            && (item.rcItem.top..item.rcItem.bottom).contains(&point.y)
    }
}

fn fill(hdc: HDC, rect: &RECT, color: u32) {
    unsafe {
        SetDCBrushColor(hdc, colorref(color));
        FillRect(hdc, rect, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}

fn round(hdc: HDC, rect: RECT, fill: u32, border: u32, diameter: i32) {
    unsafe {
        let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(DC_PEN));
        SetDCBrushColor(hdc, colorref(fill));
        SetDCPenColor(hdc, colorref(border));
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            diameter,
            diameter,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
    }
}

fn text(hdc: HDC, value: &str, mut rect: RECT, font: HFONT, color: u32, align: DRAW_TEXT_FORMAT) {
    let mut value = wide(value);
    unsafe {
        let old_font = SelectObject(hdc, HGDIOBJ(font.0));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, colorref(color));
        DrawTextW(
            hdc,
            &mut value,
            &mut rect,
            align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_font);
    }
}
