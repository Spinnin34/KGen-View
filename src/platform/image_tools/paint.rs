use super::*;
use windows::Win32::{
    Foundation::POINT,
    UI::{
        Controls::{
            COMBOBOXINFO, DRAWITEMSTRUCT, GetComboBoxInfo, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED,
            ODT_COMBOBOX, TBM_GETCHANNELRECT, TBM_GETTHUMBRECT, TBM_SETPOS,
        },
        Input::KeyboardAndMouse::GetFocus,
        Shell::{DefSubclassProc, SetWindowSubclass},
    },
};

pub(super) struct Section {
    pub rect: RECT,
    pub title: Vec<u16>,
}

pub(super) fn sections(dc: HDC, d: &Dialog) {
    let old = unsafe { SelectObject(dc, HGDIOBJ(d.font.0)) };
    let line = brush(if d.dark { 0x00555555 } else { 0x00aeaeae });
    for section in d.sections.borrow().iter() {
        let mut rect = section.rect;
        if section.title.is_empty() {
            unsafe {
                FillRect(dc, &rect, line);
            }
            continue;
        }
        rect.top += d.px(7);
        unsafe {
            FrameRect(dc, &rect, line);
            let mut extent = windows::Win32::Foundation::SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &section.title, &mut extent);
            let mut title = RECT {
                left: rect.left + d.px(8),
                right: rect.left + d.px(16) + extent.cx,
                top: section.rect.top,
                bottom: section.rect.top + d.px(16),
            };
            FillRect(dc, &title, d.background);
            title.left += d.px(4);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(if d.dark { 0x00eeeeee } else { 0x001b1b1b }));
            text(
                dc,
                title,
                &section.title,
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
        }
    }
    unsafe {
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(line.0));
    }
}

pub(super) fn install_button(hwnd: HWND, dpi: u32) {
    unsafe {
        let _ = SetWindowTheme(hwnd, w!(""), w!(""));
        let _ = SetWindowSubclass(hwnd, Some(button_proc), 1, dpi as usize);
    }
}

pub(super) fn install_combo(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(combo_proc), 1, 0);
    }
}

pub(super) fn install_trackbar(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(trackbar_proc), 1, 0);
    }
}

unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
    if matches!(msg, WM_PAINT | WM_PRINTCLIENT) {
        let dc = if msg == WM_PAINT {
            unsafe { GetDC(Some(hwnd)) }
        } else {
            HDC(wp.0 as *mut c_void)
        };
        let mut info = COMBOBOXINFO {
            cbSize: size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetComboBoxInfo(hwnd, &mut info) }.is_ok() {
            let mut rect = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &mut rect) };
            shape_frame(dc, rect, 0x00676767);
            fill(dc, &info.rcButton, 0x00343434);
            let x = (info.rcButton.left + info.rcButton.right) / 2;
            let y = (info.rcButton.top + info.rcButton.bottom) / 2;
            let radius = ((info.rcButton.right - info.rcButton.left) / 4).max(2);
            let pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(0x00e0e0e0)) };
            unsafe {
                let old = SelectObject(dc, HGDIOBJ(pen.0));
                let _ = Polyline(
                    dc,
                    &[
                        POINT {
                            x: x - radius,
                            y: y - radius / 2,
                        },
                        POINT {
                            x,
                            y: y + radius / 2,
                        },
                        POINT {
                            x: x + radius,
                            y: y - radius / 2,
                        },
                    ],
                );
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
        }
        if msg == WM_PAINT {
            unsafe {
                ReleaseDC(Some(hwnd), dc);
            }
        }
    }
    result
}

unsafe extern "system" fn trackbar_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            draw_trackbar(hwnd, dc);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            draw_trackbar(hwnd, HDC(wp.0 as *mut c_void));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
            if matches!(
                msg,
                TBM_SETPOS
                    | WM_MOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_KEYDOWN
                    | WM_SETFOCUS
                    | WM_KILLFOCUS
            ) {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            result
        }
    }
}

fn draw_trackbar(hwnd: HWND, dc: HDC) {
    let mut client = RECT::default();
    let mut line = RECT::default();
    let mut thumb = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
        SendMessageW(
            hwnd,
            TBM_GETCHANNELRECT,
            None,
            Some(LPARAM((&mut line as *mut RECT) as isize)),
        );
        SendMessageW(
            hwnd,
            TBM_GETTHUMBRECT,
            None,
            Some(LPARAM((&mut thumb as *mut RECT) as isize)),
        );
    }
    fill(dc, &client, 0x00202020);
    fill(dc, &line, 0x00535353);
    fill(dc, &thumb, 0x00e5b163);
    if unsafe { GetFocus() } == hwnd {
        shape_frame(dc, client, 0x00676767);
    }
}

fn shape_frame(dc: HDC, rect: RECT, color: u32) {
    let border = brush(color);
    unsafe {
        FrameRect(dc, &rect, border);
        let _ = DeleteObject(HGDIOBJ(border.0));
    }
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    dpi: usize,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            draw_button(hwnd, dc, dpi);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            draw_button(hwnd, HDC(wp.0 as *mut c_void), dpi);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
            if matches!(
                msg,
                BM_SETCHECK
                    | BM_SETSTATE
                    | BM_SETSTYLE
                    | WM_ENABLE
                    | WM_SETTEXT
                    | WM_SETFOCUS
                    | WM_KILLFOCUS
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_KEYDOWN
                    | WM_KEYUP
                    | WM_CAPTURECHANGED
                    | WM_CANCELMODE
            ) {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            result
        }
    }
}

fn draw_button(hwnd: HWND, dc: HDC, dpi: usize) {
    let px = |n: usize| ((n * dpi + 48) / 96) as i32;
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    let kind = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as i32 & 0xf;
    let radio = matches!(kind, BS_RADIOBUTTON | BS_AUTORADIOBUTTON);
    let toggle = radio || matches!(kind, BS_CHECKBOX | BS_AUTOCHECKBOX);
    let active = unsafe { IsWindowEnabled(hwnd) }.as_bool();
    let selected = send(hwnd, BM_GETCHECK, 0) == 1;
    let pressed = send(hwnd, BM_GETSTATE, 0) & 4 != 0;
    let focused = unsafe { GetFocus() } == hwnd;
    let face = match (toggle, active, pressed) {
        (true, _, _) => 0x00202020,
        (false, false, _) => 0x00262626,
        (false, true, true) => 0x00474747,
        _ => 0x00343434,
    };
    fill(dc, &rect, face);
    let accent = 0x00e5b163;
    let edge = if active && (focused || kind == BS_DEFPUSHBUTTON) {
        accent
    } else {
        0x00676767
    };
    let mut label = rect;
    if toggle {
        let side = px(13).max(2);
        let y = (rect.bottom - side) / 2;
        let icon = RECT {
            left: px(2),
            top: y,
            right: px(2) + side,
            bottom: y + side,
        };
        let dot = if selected && active {
            accent
        } else {
            0x00828282
        };
        if radio {
            shape(dc, icon, true, face, dot);
            if selected {
                let inner = px(4);
                shape(
                    dc,
                    RECT {
                        left: icon.left + inner,
                        top: icon.top + inner,
                        right: icon.right - inner,
                        bottom: icon.bottom - inner,
                    },
                    true,
                    dot,
                    dot,
                );
            }
        } else {
            shape(dc, icon, false, if selected { dot } else { face }, dot);
            if selected {
                let pen = unsafe { CreatePen(PS_SOLID, px(1).max(1), COLORREF(face)) };
                unsafe {
                    let old = SelectObject(dc, HGDIOBJ(pen.0));
                    let _ = Polyline(
                        dc,
                        &[
                            POINT {
                                x: icon.left + px(3),
                                y: icon.top + px(6),
                            },
                            POINT {
                                x: icon.left + px(6),
                                y: icon.top + px(9),
                            },
                            POINT {
                                x: icon.left + px(10),
                                y: icon.top + px(3),
                            },
                        ],
                    );
                    SelectObject(dc, old);
                    let _ = DeleteObject(HGDIOBJ(pen.0));
                }
            }
        }
        label.left = icon.right + px(5);
    } else {
        let border = brush(edge);
        unsafe {
            FrameRect(dc, &rect, border);
            let _ = DeleteObject(HGDIOBJ(border.0));
        }
        label.left += px(3);
        label.right -= px(3);
    }
    let mut caption = [0u16; 160];
    let n = unsafe { GetWindowTextW(hwnd, &mut caption) }.max(0) as usize;
    let font = send(hwnd, WM_GETFONT, 0);
    unsafe {
        let old = SelectObject(dc, HGDIOBJ(font as *mut c_void));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(if active { 0x00eeeeee } else { 0x00989898 }));
        let align = if toggle { DT_LEFT } else { DT_CENTER };
        text(
            dc,
            label,
            &caption[..n],
            align | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        if focused && toggle {
            label.left -= 1;
            let _ = DrawFocusRect(dc, &label);
        }
        SelectObject(dc, old);
    }
}

pub(super) fn combo_item(item: &DRAWITEMSTRUCT) -> bool {
    if item.CtlType != ODT_COMBOBOX || item.itemID == u32::MAX {
        return false;
    }
    let hwnd = item.hwndItem;
    let dc = item.hDC;
    let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let length = send(hwnd, CB_GETLBTEXTLEN, item.itemID as usize);
    if !(0..=1024).contains(&length) {
        return false;
    }
    let mut value = vec![0u16; length as usize + 1];
    unsafe {
        SendMessageW(
            hwnd,
            CB_GETLBTEXT,
            Some(WPARAM(item.itemID as usize)),
            Some(LPARAM(value.as_mut_ptr() as isize)),
        );
    }
    fill(
        dc,
        &item.rcItem,
        if selected { 0x00614028 } else { 0x002a2a2a },
    );
    let font = send(hwnd, WM_GETFONT, 0);
    let mut label = item.rcItem;
    label.left += 4;
    unsafe {
        let old = SelectObject(dc, HGDIOBJ(font as *mut c_void));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(if disabled { 0x00989898 } else { 0x00eeeeee }));
        text(
            dc,
            label,
            &value[..length as usize],
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            let _ = DrawFocusRect(dc, &item.rcItem);
        }
        SelectObject(dc, old);
    }
    true
}

fn brush(color: u32) -> HBRUSH {
    unsafe { CreateSolidBrush(COLORREF(color)) }
}

pub(super) fn fill(dc: HDC, rect: &RECT, color: u32) {
    let brush = brush(color);
    unsafe {
        FillRect(dc, rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

fn shape(dc: HDC, rect: RECT, round: bool, fill: u32, edge: u32) {
    let brush = brush(fill);
    let pen = unsafe { CreatePen(PS_SOLID, 1, COLORREF(edge)) };
    unsafe {
        let a = SelectObject(dc, HGDIOBJ(brush.0));
        let b = SelectObject(dc, HGDIOBJ(pen.0));
        let draw = if round { Ellipse } else { Rectangle };
        let _ = draw(dc, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(dc, b);
        SelectObject(dc, a);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

unsafe fn text(dc: HDC, mut rect: RECT, label: &[u16], flags: DRAW_TEXT_FORMAT) {
    // DrawText without DT_MODIFYSTRING does not modify the supplied text.
    let mut label = label.to_vec();
    unsafe {
        DrawTextW(dc, &mut label, &mut rect, flags);
    }
}
