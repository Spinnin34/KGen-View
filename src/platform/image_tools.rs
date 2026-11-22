use std::{cell::RefCell, ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_SETLIMITTEXT, EM_SETSEL, SetWindowTheme},
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    i18n::{Language, Text, tr},
    imaging::{EditOperation, Histogram},
    platform::{text::wide, theme::apply_window_theme},
};

mod histogram;
mod size;
pub use size::SizeTool;

const CLASS: PCWSTR = w!("KGenViewImageTools");
const WIDTH: i32 = 536;
const ACCEPT: i32 = IDOK.0;
const CANCEL: i32 = IDCANCEL.0;

pub fn show_size(
    owner: HWND,
    language: Language,
    dark: bool,
    tool: SizeTool,
    dimensions: (u32, u32),
) -> Option<EditOperation> {
    let text = match tool {
        SizeTool::Canvas => Text::CanvasSize,
        SizeTool::Resize => Text::ResizeImage,
    };
    let body = Body::Size(size::Panel::new(tool, dimensions));
    show(owner, language, dark, text, body)
}

pub fn show_histogram(owner: HWND, language: Language, dark: bool, data: Box<Histogram>) {
    let body = Body::Histogram(histogram::Panel::new(data));
    show(owner, language, dark, Text::Histogram, body);
}

enum Body {
    Size(size::Panel),
    Histogram(histogram::Panel),
}

struct Dialog {
    body: RefCell<Body>,
    language: Language,
    dark: bool,
    dpi: u32,
    background: HBRUSH,
    input: HBRUSH,
    font: HFONT,
}

impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.background.0));
            let _ = DeleteObject(HGDIOBJ(self.input.0));
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}

fn show(
    owner: HWND,
    language: Language,
    dark: bool,
    title: Text,
    body: Body,
) -> Option<EditOperation> {
    let instance = unsafe { GetModuleHandleW(None).ok()? }.into();
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            hInstance: instance,
            lpszClassName: CLASS,
            lpfnWndProc: Some(window_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
    }
    let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let height = match &body {
        Body::Size(_) => size::HEIGHT,
        Body::Histogram(_) => histogram::HEIGHT,
    };
    let (background, input) = colors(dark);
    let dialog = Box::new(Dialog {
        body: RefCell::new(body),
        language,
        dark,
        dpi,
        background: unsafe { CreateSolidBrush(background) },
        input: unsafe { CreateSolidBrush(input) },
        font: unsafe {
            CreateFontW(
                -(14 * dpi as i32 / 96),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            )
        },
    });
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
    let mut rect = dialog.rect(0, 0, WIDTH, height);
    unsafe { AdjustWindowRectExForDpi(&mut rect, style, false, ex_style, dpi).ok()? };
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let mut owner_rect = RECT::default();
    let monitor = unsafe { MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetWindowRect(owner, &mut owner_rect);
        let _ = GetMonitorInfoW(monitor, &mut monitor_info);
    }
    let work = monitor_info.rcWork;
    let x = ((owner_rect.left + owner_rect.right - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let y = ((owner_rect.top + owner_rect.bottom - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));
    let title = wide(tr(language, title).trim_end_matches('…'));
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            CLASS,
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(std::ptr::from_ref(dialog.as_ref()).cast()),
        )
        .ok()?
    };
    let focus = {
        let mut body = dialog.body.borrow_mut();
        match &mut *body {
            Body::Size(panel) => panel.create(hwnd, &dialog),
            Body::Histogram(panel) => panel.create(hwnd, &dialog),
        }
    };
    unsafe {
        apply_window_theme(hwnd, dark);
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(focus));
    }
    let mut message = MSG::default();
    while unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        let received = unsafe { GetMessageW(&mut message, Some(hwnd), 0, 0) };
        if received.0 <= 0 {
            if received.0 == 0 {
                unsafe { PostQuitMessage(message.wParam.0 as i32) };
            }
            break;
        }
        if !unsafe { IsDialogMessageW(hwnd, &message) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
    }
    match &*dialog.body.borrow() {
        Body::Size(panel) => panel.result,
        Body::Histogram(_) => None,
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize) };
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Dialog;
    let Some(dialog) = (unsafe { pointer.as_ref() }) else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    match message {
        WM_CLOSE | WM_COMMAND if message == WM_CLOSE || wparam.0 & 0xffff == CANCEL as usize => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        DM_GETDEFID => LRESULT(((DC_HASDEFID << 16) | ACCEPT as u32) as isize),
        WM_COMMAND => {
            if let Ok(mut body) = dialog.body.try_borrow_mut() {
                let id = (wparam.0 & 0xffff) as i32;
                let notification = (wparam.0 >> 16) as u32;
                let close = match &mut *body {
                    Body::Size(panel) => panel.command(hwnd, dialog, id, notification),
                    Body::Histogram(panel) => panel.command(hwnd, dialog, id),
                };
                if close {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_HSCROLL | WM_MOUSEMOVE => {
            if let Ok(mut body) = dialog.body.try_borrow_mut()
                && let Body::Histogram(panel) = &mut *body
            {
                panel.interact(hwnd, dialog, message, lparam);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut paint) };
            if let Ok(body) = dialog.body.try_borrow()
                && let Body::Histogram(panel) = &*body
            {
                panel.paint(dc, dialog);
            }
            unsafe {
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN => {
            let dc = HDC(wparam.0 as *mut c_void);
            let (background, input) = colors(dialog.dark);
            let is_input = matches!(message, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX);
            unsafe {
                SetTextColor(
                    dc,
                    if dialog.dark {
                        COLORREF(0x00eeeeee)
                    } else {
                        COLORREF(0x001b1b1b)
                    },
                );
                SetBkColor(dc, if is_input { input } else { background });
            }
            LRESULT(if is_input {
                dialog.input.0
            } else {
                dialog.background.0
            } as isize)
        }
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                FillRect(HDC(wparam.0 as *mut c_void), &rect, dialog.background);
            }
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn colors(dark: bool) -> (COLORREF, COLORREF) {
    if dark {
        (COLORREF(0x00201e1c), COLORREF(0x002e2c2a))
    } else {
        (COLORREF(0x00f7f7f7), COLORREF(0x00ffffff))
    }
}

impl Dialog {
    fn text<'a>(&self, spanish: &'a str, english: &'a str) -> &'a str {
        match self.language {
            Language::Spanish => spanish,
            Language::English => english,
        }
    }

    fn px(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
    }

    fn rect(&self, x: i32, y: i32, w: i32, h: i32) -> RECT {
        RECT {
            left: self.px(x),
            top: self.px(y),
            right: self.px(x + w),
            bottom: self.px(y + h),
        }
    }

    fn control(
        &self,
        owner: HWND,
        id: i32,
        class: PCWSTR,
        title: &str,
        style: WINDOW_STYLE,
        [x, y, width, height]: [i32; 4],
    ) -> HWND {
        let title = wide(title);
        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(title.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                self.px(x),
                self.px(y),
                self.px(width),
                self.px(height),
                Some(owner),
                (id != 0).then_some(HMENU(id as usize as *mut c_void)),
                None,
                None,
            )
        }
        .unwrap_or_default();
        if !handle.0.is_null() {
            unsafe {
                SendMessageW(
                    handle,
                    WM_SETFONT,
                    Some(WPARAM(self.font.0 as usize)),
                    Some(LPARAM(1)),
                );
                let theme = if self.dark {
                    w!("DarkMode_Explorer")
                } else {
                    w!("Explorer")
                };
                let _ = SetWindowTheme(handle, theme, None);
            }
        }
        handle
    }

    fn label(&self, owner: HWND, id: i32, title: &str, bounds: [i32; 4]) -> HWND {
        self.control(
            owner,
            id,
            w!("STATIC"),
            title,
            WINDOW_STYLE::default(),
            bounds,
        )
    }

    fn input(&self, owner: HWND, id: i32, value: &str, bounds: [i32; 4]) -> HWND {
        let handle = self.control(
            owner,
            id,
            w!("EDIT"),
            value,
            WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            bounds,
        );
        send(handle, EM_SETLIMITTEXT, 24);
        handle
    }

    fn combo(
        &self,
        owner: HWND,
        id: i32,
        items: &[&str],
        selected: usize,
        bounds: [i32; 4],
    ) -> HWND {
        let handle = self.control(
            owner,
            id,
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            bounds,
        );
        for item in items {
            let item = wide(item);
            unsafe {
                SendMessageW(
                    handle,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(item.as_ptr() as isize)),
                );
            }
        }
        send(handle, CB_SETCURSEL, selected);
        handle
    }

    fn button(&self, owner: HWND, id: i32, title: &str, style: i32, bounds: [i32; 4]) -> HWND {
        self.control(
            owner,
            id,
            w!("BUTTON"),
            title,
            WS_TABSTOP | WINDOW_STYLE(style as u32),
            bounds,
        )
    }

    fn checkbox(&self, owner: HWND, id: i32, title: &str, checked: bool, bounds: [i32; 4]) -> HWND {
        let handle = self.button(owner, id, title, BS_AUTOCHECKBOX, bounds);
        send(handle, BM_SETCHECK, usize::from(checked));
        handle
    }
}

fn control(hwnd: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(Some(hwnd), id) }.unwrap_or_default()
}

fn send(hwnd: HWND, message: u32, value: usize) -> isize {
    if hwnd.0.is_null() {
        return -1;
    }
    unsafe { SendMessageW(hwnd, message, Some(WPARAM(value)), None) }.0
}

fn selected(hwnd: HWND, id: i32) -> usize {
    send(control(hwnd, id), CB_GETCURSEL, 0).max(0) as usize
}

fn checked(hwnd: HWND, id: i32) -> bool {
    send(control(hwnd, id), BM_GETCHECK, 0) == 1
}

fn value(hwnd: HWND, id: i32) -> String {
    let mut buffer = [0; 48];
    let read = unsafe { GetWindowTextW(control(hwnd, id), &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..read])
}

fn set_text(hwnd: HWND, text: &str) {
    let text = wide(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }
}
