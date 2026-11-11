use super::*;

pub(super) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize) };
    }
    let pointer = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
            as *mut DialogState
    };
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_CREATE => {
            unsafe { create_controls(hwnd, state) };
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 & 0xffff) == ID_CONVERT => {
            unsafe { begin_conversion(hwnd, state) };
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 & 0xffff) == ID_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 & 0xffff) == ID_FORMAT_ARROW => {
            unsafe {
                SendMessageW(
                    state.format_combo,
                    CB_SHOWDROPDOWN,
                    Some(WPARAM(1)),
                    Some(LPARAM(0)),
                );
            }
            LRESULT(0)
        }
        WM_MEASUREITEM => {
            let measure = lparam.0 as *mut MEASUREITEMSTRUCT;
            if let Some(measure) = unsafe { measure.as_mut() }
                && measure.CtlType == DRAWITEMSTRUCT_CTL_TYPE(ODT_COMBOBOX)
            {
                measure.itemHeight =
                    scaled(34, unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0) as u32;
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_DRAWITEM => {
            let item = lparam.0 as *const DRAWITEMSTRUCT;
            if let Some(item) = unsafe { item.as_ref() } {
                unsafe { draw_item(item, state) };
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_NCHITTEST => {
            let mut point = POINT {
                x: (lparam.0 as u32 as u16 as i16) as i32,
                y: ((lparam.0 as u32 >> 16) as u16 as i16) as i32,
            };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut point);
                if point.y < scaled(48, GetDpiForWindow(hwnd).max(96) as f32 / 96.0) {
                    LRESULT(HTCAPTION as isize)
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
        }
        WM_TIMER if wparam.0 == TIMER_DOTS => {
            state.dots = (state.dots + 1) % 4;
            let label = if state.language == Language::Spanish {
                "Convirtiendo"
            } else {
                "Converting"
            };
            let text = wide(&format!("{label}{}", ".".repeat(state.dots + 1)));
            unsafe {
                let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
            }
            LRESULT(0)
        }
        WM_CONVERSION_DONE => {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_DOTS);
            }
            let result = state
                .completion
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            state.busy = false;
            unsafe {
                let _ = EnableWindow(state.format_combo, true);
                let _ = EnableWindow(state.format_arrow_button, true);
                let _ = EnableWindow(state.convert_button, true);
            }
            let message = match result {
                Some(Ok(path)) => {
                    let label = if state.language == Language::Spanish {
                        "Guardado en"
                    } else {
                        "Saved to"
                    };
                    format!("{label}: {}", compact_path(&path))
                }
                Some(Err(error)) => error,
                None => {
                    if state.language == Language::Spanish {
                        "No se recibió el resultado.".into()
                    } else {
                        "No result was received.".into()
                    }
                }
            };
            let text = wide(&message);
            unsafe {
                let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            let control = HWND(lparam.0 as *mut c_void);
            let color = if control == state.hint
                || control == state.file_label
                || control == state.format_label
            {
                state.muted
            } else {
                state.foreground
            };
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, color);
            }
            LRESULT(state.background.0 as isize)
        }
        WM_ERASEBKGND => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                FillRect(hdc, &rect, state.background);
                let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
                FillRect(hdc, &scaled_rect(0, 0, 600, 46, scale), state.header);
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: scaled(46, scale),
                        right: rect.right,
                        bottom: scaled(49, scale),
                    },
                    state.accent,
                );
                let source = scaled_rect(28, 151, 572, 210, scale);
                FillRect(hdc, &source, state.raised);
                FrameRect(hdc, &source, state.border);
                FillRect(hdc, &scaled_rect(28, 151, 32, 210, scale), state.accent);
                let status = scaled_rect(28, 305, 572, 358, scale);
                FillRect(hdc, &status, state.raised);
                FrameRect(hdc, &status, state.border);
                FillRect(hdc, &scaled_rect(28, 305, 32, 358, scale), state.accent);
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
            let mut title = wide("KGen View");
            let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            let mut rect = scaled_rect(28, 8, 165, 40, scale);
            let mut section = wide("CONVERSIÓN DE IMAGEN");
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, state.foreground);
                let old_font = SelectObject(hdc, HGDIOBJ(state.heading_font.0));
                DrawTextW(
                    hdc,
                    &mut title,
                    &mut rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
                SelectObject(hdc, old_font);
                SetTextColor(hdc, state.accent_color);
                let mut section_rect = scaled_rect(180, 10, 350, 38, scale);
                DrawTextW(
                    hdc,
                    &mut section,
                    &mut section_rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe {
                let _ = PostMessageW(
                    Some(state.owner),
                    WM_CONVERSION_CLOSED,
                    WPARAM(0),
                    LPARAM(hwnd.0 as isize),
                );
            }
            state.destroyed.store(true, Ordering::Release);
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                destroy_state(pointer);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
