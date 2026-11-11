use std::{
    ffi::c_void,
    mem::size_of,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
            DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DT_END_ELLIPSIS, DT_LEFT,
            DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_BOLD,
            FillRect, FrameRect, GetMonitorInfoW, GetStockObject, HBRUSH, HFONT, HGDIOBJ,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, OUT_DEFAULT_PRECIS,
            PAINTSTRUCT, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{
            Controls::{
                DRAWITEMSTRUCT, DRAWITEMSTRUCT_CTL_TYPE, MEASUREITEMSTRUCT, ODS_COMBOBOXEDIT,
                SetWindowTheme,
            },
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::{
                BS_OWNERDRAW, CB_ADDSTRING, CB_GETCURSEL, CB_GETLBTEXT, CB_SETCURSEL,
                CB_SHOWDROPDOWN, CBS_DROPDOWNLIST, CBS_HASSTRINGS, CBS_OWNERDRAWFIXED,
                CREATESTRUCTW, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
                HMENU, HTCAPTION, IDC_ARROW, LoadCursorW, PostMessageW, RegisterClassExW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
                ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
                WM_CTLCOLORSTATIC, WM_DRAWITEM, WM_ERASEBKGND, WM_MEASUREITEM, WM_NCCREATE,
                WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSEXW, WS_CHILD,
                WS_EX_TOOLWINDOW, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    i18n::Language,
    imaging::{ExportFormat, ImageDocument},
    platform::{text::wide, theme::apply_window_theme},
};

mod controls;
mod window;
mod worker;

use controls::{create_controls, draw_item};
use window::window_proc;
use worker::begin_conversion;

const CLASS_NAME: PCWSTR = w!("KGenView.ConversionWindow");
const ID_FORMAT: usize = 1;
const ID_CONVERT: usize = 2;
const ID_CLOSE: usize = 3;
const ID_FORMAT_ARROW: usize = 5;
const TIMER_DOTS: usize = 1;
const WM_CONVERSION_DONE: u32 = 0x8000 + 16;
pub const WM_CONVERSION_CLOSED: u32 = 0x8000 + 17;
const ODT_COMBOBOX: u32 = 3;
const ODT_BUTTON: u32 = 4;
const ODS_SELECTED: u32 = 0x0001;

struct DialogState {
    dark: bool,
    language: Language,
    owner: HWND,
    source_path: PathBuf,
    snapshot: Arc<ImageDocument>,
    formats: Vec<ExportFormat>,
    selected_format: usize,
    completion: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    busy: bool,
    dots: usize,
    format_combo: HWND,
    format_arrow_button: HWND,
    hint: HWND,
    file_label: HWND,
    format_label: HWND,
    status: HWND,
    convert_button: HWND,
    background: HBRUSH,
    header: HBRUSH,
    accent: HBRUSH,
    accent_color: COLORREF,
    raised_color: COLORREF,
    foreground: COLORREF,
    heading_font: HFONT,
    muted: COLORREF,
    raised: HBRUSH,
    border: HBRUSH,
    destroyed: Arc<AtomicBool>,
}

pub fn show(
    owner: HWND,
    dark: bool,
    language: Language,
    document: &ImageDocument,
    formats: Vec<ExportFormat>,
    selected_format: usize,
) -> Option<HWND> {
    let instance = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(_) => return None,
    };
    let cursor = match unsafe { LoadCursorW(None, IDC_ARROW) } {
        Ok(cursor) => cursor,
        Err(_) => return None,
    };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) };

    let (background, surface, raised, accent, foreground, muted, border) = if dark {
        (
            0x00202020, 0x00181818, 0x00242424, 0x002A8AFF, 0x00F1EBE8, 0x00B7B1AE, 0x00353535,
        )
    } else {
        (
            0x00F2F3F5, 0x00FAFAFB, 0x00EEF0F3, 0x001667EA, 0x00201A17, 0x005A5551, 0x00D2D6DE,
        )
    };
    let destroyed = Arc::new(AtomicBool::new(false));
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let state = Box::new(DialogState {
        dark,
        language,
        owner,
        source_path: document.path.clone(),
        snapshot: Arc::new(document.export_snapshot()),
        formats,
        selected_format,
        completion: Arc::new(Mutex::new(None)),
        busy: false,
        dots: 0,
        format_combo: HWND::default(),
        format_arrow_button: HWND::default(),
        hint: HWND::default(),
        file_label: HWND::default(),
        format_label: HWND::default(),
        status: HWND::default(),
        convert_button: HWND::default(),
        background: unsafe { CreateSolidBrush(COLORREF(background)) },
        accent: unsafe { CreateSolidBrush(COLORREF(accent)) },
        foreground: COLORREF(foreground),
        heading_font: unsafe {
            CreateFontW(
                -scaled(20, scale),
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
                w!("Segoe UI"),
            )
        },
        muted: COLORREF(muted),
        accent_color: COLORREF(accent),
        raised_color: COLORREF(raised),
        header: unsafe { CreateSolidBrush(COLORREF(surface)) },
        raised: unsafe { CreateSolidBrush(COLORREF(raised)) },
        border: unsafe { CreateSolidBrush(COLORREF(border)) },
        destroyed: destroyed.clone(),
    });
    let pointer = Box::into_raw(state);
    let width = (600.0 * scale).round() as i32;
    let height = (420.0 * scale).round() as i32;
    let title = wide(if language == Language::Spanish {
        "Convertir formato · KGen View"
    } else {
        "Convert image · KGen View"
    });
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(pointer.cast()),
        )
    } {
        Ok(hwnd) => hwnd,
        Err(_) => {
            if !destroyed.load(Ordering::Acquire) {
                unsafe { destroy_state(pointer) };
            }
            return None;
        }
    };

    unsafe {
        apply_window_theme(hwnd, dark);
        center_on_owner(hwnd, owner, width, height);
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    Some(hwnd)
}

fn compact_path(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= 60 {
        value.into_owned()
    } else {
        let beginning: String = characters.iter().take(24).collect();
        let ending: String = characters.iter().skip(characters.len() - 35).collect();
        format!("{beginning}…{ending}")
    }
}

fn scaled_rect(left: i32, top: i32, right: i32, bottom: i32, scale: f32) -> RECT {
    RECT {
        left: scaled(left, scale),
        top: scaled(top, scale),
        right: scaled(right, scale),
        bottom: scaled(bottom, scale),
    }
}

fn scaled(value: i32, scale: f32) -> i32 {
    (value as f32 * scale).round() as i32
}

unsafe fn center_on_owner(hwnd: HWND, owner: HWND, width: i32, height: i32) {
    let mut parent = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(owner, &mut parent);
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let has_monitor = GetMonitorInfoW(monitor, &mut info).as_bool();
        let bounds = if has_monitor {
            info.rcWork
        } else {
            RECT::default()
        };
        let centered_x = parent.left + ((parent.right - parent.left - width) / 2);
        let centered_y = parent.top + ((parent.bottom - parent.top - height) / 2);
        let x = if has_monitor {
            centered_x.clamp(bounds.left, (bounds.right - width).max(bounds.left))
        } else {
            centered_x
        };
        let y = if has_monitor {
            centered_y.clamp(bounds.top, (bounds.bottom - height).max(bounds.top))
        } else {
            centered_y
        };
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
        );
    }
}

unsafe fn destroy_state(pointer: *mut DialogState) {
    if pointer.is_null() {
        return;
    }
    let state = unsafe { Box::from_raw(pointer) };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(state.background.0));
        let _ = DeleteObject(HGDIOBJ(state.header.0));
        let _ = DeleteObject(HGDIOBJ(state.accent.0));
        let _ = DeleteObject(HGDIOBJ(state.heading_font.0));
    }
}
