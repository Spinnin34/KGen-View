use super::*;

pub(super) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        let pointer = create.lpCreateParams.cast::<DialogState>();
        // The HWND owns one reference until WM_NCDESTROY.
        unsafe {
            Rc::increment_strong_count(pointer);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const DialogState;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // Sending a native message or showing the save dialog can reenter this procedure.
    let state = unsafe {
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    match message {
        WM_CREATE => match create_controls(hwnd, &state) {
            Ok(controls) => {
                state.controls.set(controls);
                position_controls(hwnd, &state);
                unsafe {
                    SendMessageW(
                        controls.formats,
                        LB_SETCURSEL,
                        Some(WPARAM(state.initial_format)),
                        None,
                    );
                }
                LRESULT(0)
            }
            Err(_) => LRESULT(-1),
        },
        WM_COMMAND if (wparam.0 & 0xffff) == ID_CONVERT => {
            begin_conversion(hwnd, &state);
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 & 0xffff) == ID_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DRAWITEM => {
            if let Some(item) = unsafe { (lparam.0 as *const DRAWITEMSTRUCT).as_ref() } {
                draw_item(item, &state);
            }
            LRESULT(1)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            let work = work_area(suggested);
            let layout = Layout::new((wparam.0 & 0xffff) as u32, work, state.formats.len());
            let old_fonts = state.fonts.replace(Fonts::new(layout));
            state.layout.set(layout);
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested
                        .left
                        .clamp(work.left, (work.right - layout.width()).max(work.left)),
                    suggested
                        .top
                        .clamp(work.top, (work.bottom - layout.height()).max(work.top)),
                    layout.width(),
                    layout.height(),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            if !state.controls.get().formats.0.is_null() {
                position_controls(hwnd, &state);
            }
            drop(old_fonts);
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let mut point = POINT {
                x: lparam.0 as i16 as i32,
                y: (lparam.0 >> 16) as i16 as i32,
            };
            if unsafe { ScreenToClient(hwnd, &mut point) }.as_bool()
                && (0..state.layout.get().px(41)).contains(&point.y)
            {
                LRESULT(HTCAPTION as isize)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_TIMER if wparam.0 == TIMER_CONVERSION => {
            conversion_tick(hwnd, &state, true);
            LRESULT(0)
        }
        WM_CONVERSION_DONE => {
            conversion_tick(hwnd, &state, false);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            let control = HWND(lparam.0 as *mut _);
            let controls = state.controls.get();
            let text_color = if control == controls.status {
                state.status_color.get()
            } else if control == controls.formats || control == controls.source_name {
                state.palette.text
            } else {
                state.palette.muted
            };
            let surface = colorref(state.palette.surface);
            unsafe {
                SetBkColor(hdc, surface);
                SetDCBrushColor(hdc, surface);
                SetTextColor(hdc, colorref(text_color));
                LRESULT(GetStockObject(DC_BRUSH).0 as isize)
            }
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd, &state);
            LRESULT(0)
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = PostMessageW(
                Some(state.owner),
                WM_CONVERSION_CLOSED,
                WPARAM(0),
                LPARAM(hwnd.0 as isize),
            );
            drop(Rc::from_raw(pointer));
            DefWindowProcW(hwnd, message, wparam, lparam)
        },
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
