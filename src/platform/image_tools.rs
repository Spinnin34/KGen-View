use std::{cell::RefCell, ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_SETLIMITTEXT, EM_SETSEL, SetWindowTheme},
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled, SetFocus},
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
mod properties;
mod size;
pub use properties::{PropertiesData, PropertiesEdit};
pub use size::SizeTool;

const CLASS: PCWSTR = w!("KGenViewImageTools");
const ACCEPT: i32 = IDOK.0;
const CANCEL: i32 = IDCANCEL.0;
const SS_ETCHEDHORZ: WINDOW_STYLE = WINDOW_STYLE(0x10);
const SS_NOPREFIX: WINDOW_STYLE = WINDOW_STYLE(0x80);

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
    match show(
        owner,
        language,
        dark,
        tr(language, text).trim_end_matches('…'),
        body,
    )? {
        DialogResult::Size(edit) => Some(edit),
        DialogResult::Properties(_) => None,
    }
}

pub fn show_histogram(owner: HWND, language: Language, dark: bool, data: Box<Histogram>) {
    let body = Body::Histogram(histogram::Panel::new(data));
    show(
        owner,
        language,
        dark,
        tr(language, Text::Histogram).trim_end_matches('…'),
        body,
    );
}

pub fn show_properties(
    owner: HWND,
    dark: bool,
    language: Language,
    data: PropertiesData,
) -> Option<PropertiesEdit> {
    let title = match language {
        Language::Spanish => "KGen View — Propiedades de la imagen",
        Language::English => "KGen View — Image properties",
    };
    let body = Body::Properties(Box::new(properties::Panel::new(data)));
    match show(owner, language, dark, title, body)? {
        DialogResult::Properties(edit) => Some(edit),
        DialogResult::Size(_) => None,
    }
}

enum Body {
    Size(size::Panel),
    Histogram(histogram::Panel),
    Properties(Box<properties::Panel>),
}

enum DialogResult {
    Size(EditOperation),
    Properties(PropertiesEdit),
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
    title: &str,
    body: Body,
) -> Option<DialogResult> {
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
    let window_dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let (client_w, client_h) = match &body {
        Body::Size(panel) => panel.client_size(),
        Body::Histogram(_) => histogram::CLIENT,
        Body::Properties(_) => properties::CLIENT,
    };
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
    let mut frame = RECT::default();
    unsafe { AdjustWindowRectExForDpi(&mut frame, style, false, ex_style, window_dpi).ok()? };
    let (frame_w, frame_h) = (frame.right - frame.left, frame.bottom - frame.top);
    let mut owner_rect = RECT::default();
    let monitor = unsafe { MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetWindowRect(owner, &mut owner_rect).ok()?;
        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return None;
        }
    }
    let work = monitor_info.rcWork;
    let dpi = fit_layout_dpi(window_dpi, (client_w, client_h), (frame_w, frame_h), work);
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
                -(13 * dpi as i32 / 96).max(1),
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
    let (width, height) = (dialog.px(client_w) + frame_w, dialog.px(client_h) + frame_h);
    let x = ((owner_rect.left + owner_rect.right - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let y = ((owner_rect.top + owner_rect.bottom - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));
    let title = wide(title);
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
            Body::Properties(panel) => panel.create(hwnd, &dialog),
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
    match &mut *dialog.body.borrow_mut() {
        Body::Size(panel) => panel.result.take().map(DialogResult::Size),
        Body::Histogram(_) => None,
        Body::Properties(panel) => panel.result.take().map(DialogResult::Properties),
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
                    Body::Properties(panel) => panel.command(hwnd, id),
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
            let child = HWND(lparam.0 as *mut c_void);
            let (background, input) = colors(dialog.dark);
            let is_input = matches!(message, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX);
            let disabled = !unsafe { IsWindowEnabled(child).as_bool() };
            unsafe {
                SetTextColor(
                    dc,
                    COLORREF(match (dialog.dark, disabled) {
                        (true, false) => 0x00eeeeee,
                        (false, false) => 0x001b1b1b,
                        (true, true) => 0x00989898,
                        (false, true) => 0x00686868,
                    }),
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
        (COLORREF(0x00202020), COLORREF(0x002a2a2a))
    } else {
        (COLORREF(0x00f7f7f7), COLORREF(0x00ffffff))
    }
}

fn fit_layout_dpi(dpi: u32, client: (i32, i32), frame: (i32, i32), work: RECT) -> u32 {
    let available_w = (work.right - work.left - frame.0).max(1);
    let available_h = (work.bottom - work.top - frame.1).max(1);
    dpi.min(
        (available_w * 96 / client.0)
            .min(available_h * 96 / client.1)
            .max(1) as u32,
    )
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
                self.px(x + width) - self.px(x),
                self.px(y + height) - self.px(y),
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
        self.control(owner, id, w!("STATIC"), title, SS_NOPREFIX, bounds)
    }

    fn group(&self, owner: HWND, title: &str, bounds: [i32; 4]) {
        self.control(
            owner,
            0,
            w!("BUTTON"),
            title,
            WINDOW_STYLE(BS_GROUPBOX as u32) | WS_CLIPSIBLINGS,
            bounds,
        );
    }

    fn divider(&self, owner: HWND, x: i32, y: i32, width: i32) {
        self.control(owner, 0, w!("STATIC"), "", SS_ETCHEDHORZ, [x, y, width, 2]);
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

    fn radio(
        &self,
        owner: HWND,
        id: i32,
        title: &str,
        style: WINDOW_STYLE,
        bounds: [i32; 4],
    ) -> HWND {
        self.control(owner, id, w!("BUTTON"), title, style, bounds)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_properties_fit_work_area_with_window_frame_at_high_dpi() {
        let screen = RECT {
            left: -1280,
            top: 0,
            right: 0,
            bottom: 720,
        };
        let frame = (24, 58);
        let dpi = fit_layout_dpi(144, properties::CLIENT, frame, screen);
        assert!(dpi < 144);
        let pixels = |dimension| (dimension * dpi as i32 + 48) / 96;
        assert!(pixels(properties::CLIENT.0) + frame.0 <= screen.right - screen.left);
        assert!(pixels(properties::CLIENT.1) + frame.1 <= screen.bottom - screen.top);
        assert_eq!(fit_layout_dpi(96, properties::CLIENT, frame, screen), 96);
    }
}
