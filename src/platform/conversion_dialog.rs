use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED, SetWindowTheme},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                EnableWindow, GetActiveWindow, GetFocus, IsWindowEnabled, SetFocus, VK_ESCAPE,
                VK_RETURN,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    i18n::Language,
    imaging::{ExportFormat, ImageDocument},
    platform::{text::wide, theme::apply_window_theme},
    ui::theme::MenuPalette,
};

mod controls;
mod window;
mod worker;

use controls::{Controls, Fonts, create_controls, draw_item, paint, position_controls};
use window::window_proc;
use worker::{begin_conversion, conversion_tick};

const CLASS_NAME: PCWSTR = w!("KGenView.ConversionWindow");
const ID_FORMAT: usize = 1;
const ID_CONVERT: usize = 2;
const ID_CLOSE: usize = 3;
const TIMER_CONVERSION: usize = 1;
const WM_CONVERSION_DONE: u32 = 0x8000 + 16;
pub const WM_CONVERSION_CLOSED: u32 = 0x8000 + 17;

const WIDTH: i32 = 412;
const LIST_TOP: i32 = 119;
const ROW_HEIGHT: i32 = 33;
const FOOTER_HEIGHT: i32 = 66;
const CORNER_RADIUS: i32 = 8;

struct DialogState {
    dark: bool,
    language: Language,
    owner: HWND,
    snapshot: Arc<ImageDocument>,
    formats: Vec<ExportFormat>,
    initial_format: usize,
    completion: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    busy: Cell<bool>,
    dots: Cell<usize>,
    controls: Cell<Controls>,
    layout: Cell<Layout>,
    fonts: RefCell<Fonts>,
    palette: MenuPalette,
    status_color: Cell<u32>,
}

impl DialogState {
    fn label<'a>(&self, spanish: &'a str, english: &'a str) -> &'a str {
        match self.language {
            Language::Spanish => spanish,
            Language::English => english,
        }
    }

    fn title(&self) -> &str {
        self.label("Convertir formato", "Convert image format")
    }

    fn set_status(&self, status: &str, color: u32) {
        self.status_color.set(color);
        let text = wide(status);
        let control = self.controls.get().status;
        unsafe {
            let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
            let _ = InvalidateRect(Some(control), None, true);
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    scale: f32,
    visible_rows: i32,
}

impl Layout {
    fn new(dpi: u32, work: RECT, rows: usize) -> Self {
        let width = (work.right - work.left - 12).max(1) as f32;
        let scale = (dpi.max(1) as f32 / 96.0).min(width / WIDTH as f32);
        let height = (work.bottom - work.top) as f32 / scale;
        let max_rows = ((height as i32 - LIST_TOP - FOOTER_HEIGHT - 12) / ROW_HEIGHT).max(1);
        Self {
            scale,
            visible_rows: rows.min(max_rows as usize).max(1) as i32,
        }
    }

    fn px(self, value: i32) -> i32 {
        (value as f32 * self.scale).round() as i32
    }

    fn rect(self, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left: self.px(left),
            top: self.px(top),
            right: self.px(right),
            bottom: self.px(bottom),
        }
    }

    fn list_bottom(self) -> i32 {
        LIST_TOP + self.visible_rows * ROW_HEIGHT
    }

    fn width(self) -> i32 {
        self.px(WIDTH)
    }

    fn height(self) -> i32 {
        self.px(self.list_bottom() + FOOTER_HEIGHT)
    }
}

pub fn show(
    owner: HWND,
    dark: bool,
    language: Language,
    document: &ImageDocument,
    formats: Vec<ExportFormat>,
    selected_format: usize,
) -> Option<HWND> {
    let mut owner_rect = RECT::default();
    unsafe { GetWindowRect(owner, &mut owner_rect) }.ok()?;
    let work = work_area(&owner_rect);
    let layout = Layout::new(unsafe { GetDpiForWindow(owner) }, work, formats.len());
    let palette = MenuPalette::for_dark(dark);
    let state = Rc::new(DialogState {
        dark,
        language,
        owner,
        snapshot: Arc::new(document.export_snapshot()),
        initial_format: selected_format.min(formats.len().checked_sub(1)?),
        formats,
        completion: Arc::new(Mutex::new(None)),
        busy: Cell::new(false),
        dots: Cell::new(0),
        controls: Cell::new(Controls::default()),
        layout: Cell::new(layout),
        fonts: RefCell::new(Fonts::new(layout)),
        status_color: Cell::new(palette.muted),
        palette,
    });
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.ok()?.0);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.ok()?,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) };

    let title = wide(state.title());
    let width = layout.width();
    let height = layout.height();
    let center_x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
    let center_y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_CLIPCHILDREN,
            center_x.clamp(work.left, (work.right - width).max(work.left)),
            center_y.clamp(work.top, (work.bottom - height).max(work.top)),
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(Rc::as_ptr(&state).cast()),
        )
    }
    .ok()?;
    unsafe {
        apply_window_theme(hwnd, dark);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(state.controls.get().formats));
    }
    Some(hwnd)
}

pub fn tool_message(hwnd: HWND, message: &MSG) -> bool {
    if hwnd.0.is_null()
        || (hwnd != message.hwnd && !unsafe { IsChild(hwnd, message.hwnd) }.as_bool())
        || !unsafe { IsWindowEnabled(hwnd) }.as_bool()
    {
        return false;
    }
    if message.message == WM_KEYDOWN {
        let key = message.wParam.0 as u16;
        let command = match key {
            key if key == VK_ESCAPE.0 => Some(ID_CLOSE),
            key if key == VK_RETURN.0 => {
                let focus = unsafe { GetFocus() };
                let close = unsafe { GetDlgItem(Some(hwnd), ID_CLOSE as i32) };
                Some(if Ok(focus) == close {
                    ID_CLOSE
                } else {
                    ID_CONVERT
                })
            }
            _ => None,
        };
        if let Some(command) = command {
            if message.lParam.0 & (1 << 30) == 0 {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(command), LPARAM(0));
                }
            }
            return true;
        }
    }
    unsafe { IsDialogMessageW(hwnd, message) }.as_bool()
}

fn work_area(rect: &RECT) -> RECT {
    let monitor = unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        info.rcWork
    } else {
        RECT {
            right: unsafe { GetSystemMetrics(SM_CXSCREEN) },
            bottom: unsafe { GetSystemMetrics(SM_CYSCREEN) },
            ..Default::default()
        }
    }
}

fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16))
}
