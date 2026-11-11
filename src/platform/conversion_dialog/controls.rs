use super::*;

pub(super) unsafe fn create_controls(hwnd: HWND, state: &mut DialogState) {
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let theme = if state.dark {
        w!("DarkMode_Explorer")
    } else {
        w!("Explorer")
    };
    let heading = if state.language == Language::Spanish {
        "Convertir formato"
    } else {
        "Convert image format"
    };
    let hint = if state.language == Language::Spanish {
        "Crea una copia en otro formato; la imagen original se conserva."
    } else {
        "Create a copy in another format; your original image stays unchanged."
    };
    let format_label = if state.language == Language::Spanish {
        "FORMATO DE SALIDA"
    } else {
        "OUTPUT FORMAT"
    };
    let file_label = if state.language == Language::Spanish {
        "IMAGEN ABIERTA"
    } else {
        "OPEN IMAGE"
    };
    let file_name = state
        .source_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let status_label = if state.language == Language::Spanish {
        "El archivo abierto no cambiará. Solo se exportará el fotograma visible."
    } else {
        "The open image stays unchanged. Only the visible frame will be exported."
    };
    let cancel = if state.language == Language::Spanish {
        "Cerrar"
    } else {
        "Close"
    };
    let convert = if state.language == Language::Spanish {
        "Convertir"
    } else {
        "Convert"
    };

    let heading_control = create_static(hwnd, heading, 28, 66, 544, 34, scale);
    unsafe {
        SendMessageW(
            heading_control,
            WM_SETFONT,
            Some(WPARAM(state.heading_font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    state.hint = create_static(hwnd, hint, 28, 108, 544, 28, scale);
    state.file_label = create_static(hwnd, file_label, 44, 160, 510, 16, scale);
    create_static(hwnd, &file_name, 44, 179, 510, 20, scale);
    state.format_label = create_static(hwnd, format_label, 28, 225, 544, 18, scale);
    state.format_combo = create_combo(hwnd, 28, 250, 544, 166, scale);
    for format in &state.formats {
        let label = wide(&format.filter_label());
        unsafe {
            SendMessageW(
                state.format_combo,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(label.as_ptr() as isize)),
            );
        }
    }
    if !state.formats.is_empty() {
        unsafe {
            SendMessageW(
                state.format_combo,
                CB_SETCURSEL,
                Some(WPARAM(state.selected_format.min(state.formats.len() - 1))),
                Some(LPARAM(0)),
            );
        }
    }
    state.format_arrow_button = create_button(hwnd, "⌄", ID_FORMAT_ARROW, 520, 251, 52, 37, scale);
    state.status = create_static(hwnd, status_label, 44, 321, 510, 24, scale);
    let _ = create_button(hwnd, cancel, ID_CLOSE, 28, 368, 104, 36, scale);
    state.convert_button = create_button(hwnd, convert, ID_CONVERT, 436, 368, 136, 36, scale);
    let _ = unsafe { SetWindowTheme(state.format_combo, theme, None) };
}

fn create_static(
    hwnd: HWND,
    label: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
) -> HWND {
    let label = wide(label);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR(label.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            scaled(x, scale),
            scaled(y, scale),
            scaled(width, scale),
            scaled(height, scale),
            Some(hwnd),
            None,
            None,
            None,
        )
    }
    .unwrap_or_default();
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
            Some(LPARAM(1)),
        );
    }
    control
}

fn create_combo(hwnd: HWND, x: i32, y: i32, width: i32, height: i32, scale: f32) -> HWND {
    let style = WS_CHILD
        | WS_VISIBLE
        | WS_TABSTOP
        | WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | CBS_HASSTRINGS as u32 | CBS_OWNERDRAWFIXED as u32);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            w!(""),
            style,
            scaled(x, scale),
            scaled(y, scale),
            scaled(width, scale),
            scaled(height, scale),
            Some(hwnd),
            Some(HMENU(ID_FORMAT as *mut c_void)),
            None,
            None,
        )
    }
    .unwrap_or_default();
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
            Some(LPARAM(1)),
        );
    }
    control
}

fn create_button(
    hwnd: HWND,
    label: &str,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
) -> HWND {
    let label = wide(label);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            PCWSTR(label.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            scaled(x, scale),
            scaled(y, scale),
            scaled(width, scale),
            scaled(height, scale),
            Some(hwnd),
            Some(HMENU(id as *mut c_void)),
            None,
            None,
        )
    }
    .unwrap_or_default();
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
            Some(LPARAM(1)),
        );
    }
    control
}

pub(super) unsafe fn draw_item(item: &DRAWITEMSTRUCT, state: &DialogState) {
    let hdc = item.hDC;
    let selected = item.itemState.0 & ODS_SELECTED != 0;
    if item.CtlType == DRAWITEMSTRUCT_CTL_TYPE(ODT_BUTTON) {
        let id = item.CtlID as usize;
        let primary = id == ID_CONVERT;
        let arrow = id == ID_FORMAT_ARROW;
        let fill = if primary {
            if selected {
                if state.dark { 0x001A5EA8 } else { 0x001254C4 }
            } else {
                state.accent_color.0
            }
        } else if arrow && selected {
            state.accent_color.0
        } else {
            state.raised_color.0
        };
        let foreground = if primary {
            COLORREF(0x00FFFFFF)
        } else if arrow {
            state.muted
        } else {
            state.foreground
        };
        let brush = unsafe { CreateSolidBrush(COLORREF(fill)) };
        let mut rect = item.rcItem;
        unsafe {
            FillRect(hdc, &rect, brush);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, foreground);
        }
        let mut label = if arrow {
            wide("⌄")
        } else if id == ID_CONVERT {
            wide(if state.language == Language::Spanish {
                "Convertir"
            } else {
                "Convert"
            })
        } else if state.language == Language::Spanish {
            wide("Cerrar")
        } else {
            wide("Close")
        };
        unsafe {
            DrawTextW(
                hdc,
                &mut label,
                &mut rect,
                windows::Win32::Graphics::Gdi::DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if !primary && !arrow {
                FrameRect(hdc, &item.rcItem, state.border);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        return;
    }

    if item.CtlType == DRAWITEMSTRUCT_CTL_TYPE(ODT_COMBOBOX) {
        let fill = if selected {
            state.accent_color.0
        } else {
            state.raised_color.0
        };
        let foreground = if selected {
            COLORREF(0x00FFFFFF)
        } else {
            state.foreground
        };
        let brush = unsafe { CreateSolidBrush(COLORREF(fill)) };
        unsafe {
            FillRect(hdc, &item.rcItem, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, foreground);
        }
        let mut rect = item.rcItem;
        rect.left += 12;
        rect.right -= 10;
        let mut label = vec![0u16; 1024];
        if item.itemID != u32::MAX {
            let length = unsafe {
                SendMessageW(
                    state.format_combo,
                    CB_GETLBTEXT,
                    Some(WPARAM(item.itemID as usize)),
                    Some(LPARAM(label.as_mut_ptr() as isize)),
                )
            }
            .0;
            if length >= 0 {
                label.truncate((length as usize + 1).min(label.len()));
                unsafe {
                    DrawTextW(
                        hdc,
                        &mut label,
                        &mut rect,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                    );
                }
            }
        }
        if item.itemState.0 & ODS_COMBOBOXEDIT.0 != 0 {
            unsafe { FrameRect(hdc, &item.rcItem, state.border) };
        }
    }
}
