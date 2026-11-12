//! Optional, event-driven rectangular screen capture.
//!
//! The resident mode is a hidden capture-only window: it registers one global
//! hotkey and sleeps in the Win32 message loop until that key is pressed.

use std::{
    ffi::c_void,
    fs,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    Win32::{
        Foundation::{
            COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
            POINT, RECT, WPARAM,
        },
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACK_BRUSH, BeginPaint, BitBlt, CAPTUREBLT,
            CombineRgn, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateRectRgn,
            CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, FillRect, GetDC,
            GetDIBits, GetStockObject, HDC, HGDIOBJ, HOLLOW_BRUSH, InvalidateRect, PAINTSTRUCT,
            PS_SOLID, RGN_DIFF, RGN_ERROR, Rectangle, ReleaseDC, RoundRect, SRCCOPY, SelectObject,
            SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT, UpdateWindow, WHITE_PEN,
        },
        Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT},
        System::{
            Com::CoTaskMemFree,
            Diagnostics::Debug::MessageBeep,
            LibraryLoader::GetModuleHandleW,
            SystemInformation::GetSystemTime,
            Time::SystemTimeToTzSpecificLocalTime,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
        UI::{
            Controls::{
                BST_CHECKED, HKM_GETHOTKEY, HKM_SETHOTKEY, HOTKEY_CLASS, HOTKEYF_ALT,
                HOTKEYF_CONTROL, HOTKEYF_SHIFT, ICC_HOTKEY_CLASS, INITCOMMONCONTROLSEX,
                InitCommonControlsEx,
            },
            Input::KeyboardAndMouse::{
                HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
                RegisterHotKey, SetCapture, UnregisterHotKey,
            },
            Shell::{FOLDERID_Pictures, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath},
            WindowsAndMessaging::{
                BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CREATESTRUCTW,
                CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW,
                GetSystemMetrics, GetWindowLongPtrW, HMENU, IDC_ARROW, IDC_CROSS, IsDialogMessageW,
                KillTimer, LoadCursorW, MB_ICONASTERISK, MSG, MessageBoxW, PostQuitMessage,
                RegisterClassExW, SWP_SHOWWINDOW, SetForegroundWindow, SetLayeredWindowAttributes,
                SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
                WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_ERASEBKGND,
                WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
                WM_PAINT, WM_TIMER, WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
                WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    platform::clipboard::PreparedImage,
    settings::{CaptureHotkey, Settings},
};

const HOST_CLASS: PCWSTR = w!("KGenView.CaptureHost");
const HOST_TITLE: PCWSTR = w!("KGen View Capture Background");
const OVERLAY_CLASS: PCWSTR = w!("KGenView.CaptureOverlay");
const HOTKEY_DIALOG_CLASS: PCWSTR = w!("KGenView.CaptureHotkeyDialog");
const HOST_HOTKEY_ID: i32 = 0x4b47;
const CAPTURE_REDRAW_TIMER_ID: usize = 0x4b48;
const CAPTURE_REDRAW_INTERVAL_MS: u32 = 16;
const WM_CAPTURE_QUERY: u32 = WM_APP + 70;
const WM_OVERLAY_DONE: u32 = WM_APP + 71;
const HOST_RUN_ARGUMENT: &str = "--capture-background";
const APP_USER_MODEL_ID: &str = "KGen.View";

static CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct HostState {
    hotkey_registered: bool,
    spanish: bool,
}

struct OverlayState {
    desktop: RECT,
    start: Option<POINT>,
    end: POINT,
    rendered_end: Option<POINT>,
    redraw_timer_active: bool,
    cancelled: bool,
    hint: Vec<u16>,
}

struct HotkeyDialogState {
    hotkey_control: HWND,
    windows_key_control: HWND,
    initial: CaptureHotkey,
    result: Option<CaptureHotkey>,
    done: bool,
    spanish: bool,
}

/// Starts the lightweight capture-only process if it is not already running.
/// It waits only during activation; the resident process uses no polling loop.
pub fn start_background() -> Result<(), String> {
    if let Some(hwnd) = find_host() {
        return query_host(hwnd);
    }

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut child = std::process::Command::new(executable)
        .arg(HOST_RUN_ARGUMENT)
        .arg("--quiet")
        .spawn()
        .map_err(|error| format!("No se pudo iniciar la captura en segundo plano: {error}"))?;

    for _ in 0..150 {
        if let Some(hwnd) = find_host() {
            let result = unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    hwnd,
                    WM_CAPTURE_QUERY,
                    Some(WPARAM(0)),
                    Some(LPARAM(0)),
                )
            };
            if result.0 == 1 {
                return Ok(());
            }
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
        } else if child.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("Windows no pudo registrar el atajo global. Cambia el atajo de captura y vuelve a intentarlo.".into())
}

pub fn stop_background() {
    if let Some(hwnd) = find_host() {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                hwnd,
                WM_CLOSE,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            );
        }
    }
}

pub fn run_background() -> Result<(), String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let instance = HINSTANCE(module.0);
    register_window_class(
        HOST_CLASS,
        Some(host_window_proc),
        instance,
        IDC_ARROW,
        Default::default(),
    )?;
    register_window_class(
        OVERLAY_CLASS,
        Some(overlay_window_proc),
        instance,
        IDC_CROSS,
        Default::default(),
    )?;

    if find_host().is_some() {
        return Ok(());
    }

    let settings = Settings::load();
    settings.save();
    let state = Box::into_raw(Box::new(HostState {
        hotkey_registered: false,
        spanish: settings.language == crate::i18n::Language::Spanish,
    }));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            HOST_CLASS,
            HOST_TITLE,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            Some(state.cast()),
        )
    }
    .map_err(|error| {
        unsafe { drop(Box::from_raw(state)) };
        error.to_string()
    })?;

    let modifiers =
        HOT_KEY_MODIFIERS(register_modifiers(settings.capture_hotkey.modifiers).0 | MOD_NOREPEAT.0);
    if let Err(error) = unsafe {
        RegisterHotKey(
            Some(hwnd),
            HOST_HOTKEY_ID,
            modifiers,
            settings.capture_hotkey.key,
        )
    } {
        unsafe {
            let _ = DestroyWindow(hwnd);
            drop(Box::from_raw(state));
        }
        return Err(format!("No se pudo registrar el atajo global: {error}"));
    }
    unsafe { (*state).hotkey_registered = true };

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { drop(Box::from_raw(state)) };
    Ok(())
}

fn find_host() -> Option<HWND> {
    unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(HOST_CLASS, HOST_TITLE) }.ok()
}

fn query_host(hwnd: HWND) -> Result<(), String> {
    let result = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_CAPTURE_QUERY,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
    };
    if result.0 == 1 {
        Ok(())
    } else {
        Err(
            "La captura en segundo plano está activa, pero Windows no pudo registrar el atajo."
                .into(),
        )
    }
}

fn register_window_class(
    name: PCWSTR,
    procedure: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
    instance: HINSTANCE,
    cursor_name: PCWSTR,
    style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES,
) -> Result<(), String> {
    let cursor = unsafe { LoadCursorW(None, cursor_name) }.map_err(|error| error.to_string())?;
    let class = windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW {
        cbSize: size_of::<windows::Win32::UI::WindowsAndMessaging::WNDCLASSEXW>() as u32,
        style,
        lpfnWndProc: procedure,
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: name,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err("No se pudo registrar la ventana de captura.".into());
    }
    Ok(())
}

unsafe extern "system" fn host_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        let state = unsafe { (*create).lpCreateParams as *mut HostState };
        if !state.is_null() {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        }
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HostState };
    match message {
        WM_CAPTURE_QUERY => LRESULT(
            if !state.is_null() && unsafe { (*state).hotkey_registered } {
                1
            } else {
                0
            },
        ),
        WM_HOTKEY if wparam.0 as i32 == HOST_HOTKEY_ID => {
            if !CAPTURE_IN_PROGRESS.swap(true, Ordering::AcqRel) {
                let result =
                    select_and_capture(hwnd, !state.is_null() && unsafe { (*state).spanish });
                CAPTURE_IN_PROGRESS.store(false, Ordering::Release);
                if let Err(error) = result {
                    show_error(&error);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if !state.is_null() && unsafe { (*state).hotkey_registered } {
                unsafe {
                    let _ = UnregisterHotKey(Some(hwnd), HOST_HOTKEY_ID);
                }
                unsafe {
                    (*state).hotkey_registered = false;
                }
            }
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        let state = unsafe { (*create).lpCreateParams as *mut OverlayState };
        if !state.is_null() {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        }
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState };
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
            let background = RECT {
                left: 0,
                top: 0,
                right: unsafe {
                    GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN)
                },
                bottom: unsafe {
                    GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN)
                },
            };
            let black = unsafe { GetStockObject(BLACK_BRUSH) };
            unsafe {
                let _ = FillRect(
                    hdc,
                    &background,
                    windows::Win32::Graphics::Gdi::HBRUSH(black.0),
                );
            }
            if !state.is_null() {
                let state = unsafe { &*state };
                if let (Some(start), Some(end)) = (state.start, state.rendered_end) {
                    let left = start.x.min(end.x) - state.desktop.left;
                    let top = start.y.min(end.y) - state.desktop.top;
                    let right = start.x.max(end.x) - state.desktop.left;
                    let bottom = start.y.max(end.y) - state.desktop.top;
                    draw_selection_frame(hdc, left, top, right, bottom);
                }
                let hint_right =
                    24 + ((state.hint.len() as i32).saturating_mul(8) + 44).clamp(280, 470);
                let clip = paint.rcPaint;
                let hint_visible =
                    clip.left < hint_right && clip.right > 24 && clip.top < 66 && clip.bottom > 22;
                if hint_visible {
                    unsafe {
                        let _ = SetBkMode(hdc, TRANSPARENT);
                        let _ = SetTextColor(hdc, COLORREF(0x00ffffff));
                    }
                    draw_hint_chip(hdc, &state.hint);
                }
            }
            unsafe {
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !state.is_null() {
                let mut point = POINT::default();
                if unsafe { GetCursorPos(&mut point) }.is_ok() {
                    let state = unsafe { &mut *state };
                    state.start = Some(point);
                    state.end = point;
                    state.rendered_end = None;
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !state.is_null() && unsafe { (*state).start.is_some() } {
                let mut point = POINT::default();
                if unsafe { GetCursorPos(&mut point) }.is_ok() {
                    unsafe {
                        let state = &mut *state;
                        if point.x != state.end.x || point.y != state.end.y {
                            state.end = point;
                            if !state.redraw_timer_active {
                                let timer = SetTimer(
                                    Some(hwnd),
                                    CAPTURE_REDRAW_TIMER_ID,
                                    CAPTURE_REDRAW_INTERVAL_MS,
                                    None,
                                );
                                if timer == 0 {
                                    update_overlay_selection(hwnd, state, state.rendered_end);
                                    state.rendered_end = Some(state.end);
                                } else {
                                    state.redraw_timer_active = true;
                                }
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if !state.is_null() && unsafe { (*state).start.is_some() } {
                let mut point = POINT::default();
                if unsafe { GetCursorPos(&mut point) }.is_ok() {
                    unsafe {
                        let state = &mut *state;
                        let start = state.start.unwrap();
                        state.end = point;
                        if state.redraw_timer_active {
                            let _ = KillTimer(Some(hwnd), CAPTURE_REDRAW_TIMER_ID);
                            state.redraw_timer_active = false;
                        }
                        if state.rendered_end != Some(point)
                            && (state.rendered_end.is_some()
                                || point.x != start.x
                                || point.y != start.y)
                        {
                            update_overlay_selection(hwnd, state, state.rendered_end);
                            state.rendered_end = Some(point);
                        }
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(hwnd),
                            WM_OVERLAY_DONE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CAPTURE_REDRAW_TIMER_ID => {
            if !state.is_null() {
                unsafe {
                    let state = &mut *state;
                    let _ = KillTimer(Some(hwnd), CAPTURE_REDRAW_TIMER_ID);
                    state.redraw_timer_active = false;
                    let mut point = POINT::default();
                    if state.start.is_some()
                        && GetCursorPos(&mut point).is_ok()
                        && state.rendered_end != Some(point)
                    {
                        state.end = point;
                        update_overlay_selection(hwnd, state, state.rendered_end);
                        state.rendered_end = Some(point);
                    }
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN
            if wparam.0 as u32
                == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as u32 =>
        {
            if !state.is_null() {
                unsafe {
                    (*state).cancelled = true;
                    let _ = KillTimer(Some(hwnd), CAPTURE_REDRAW_TIMER_ID);
                    (*state).redraw_timer_active = false;
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                }
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd),
                        WM_OVERLAY_DONE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if !state.is_null() {
                unsafe {
                    (*state).cancelled = true;
                    let _ = KillTimer(Some(hwnd), CAPTURE_REDRAW_TIMER_ID);
                    (*state).redraw_timer_active = false;
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                }
            }
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_OVERLAY_DONE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn draw_selection_frame(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) {
    let width = right - left;
    let height = bottom - top;
    if width < 2 || height < 2 {
        return;
    }
    unsafe {
        let old_brush = SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(WHITE_PEN));
        let _ = Rectangle(hdc, left - 1, top - 1, right + 1, bottom + 1);
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
    }
}

fn draw_hint_chip(hdc: HDC, hint: &[u16]) {
    let width = ((hint.len() as i32).saturating_mul(8) + 44).clamp(280, 470);
    let chip = RECT {
        left: 24,
        top: 22,
        right: 24 + width,
        bottom: 66,
    };
    unsafe {
        let fill = CreateSolidBrush(COLORREF(0x0028201b));
        let border = CreatePen(PS_SOLID, 1, COLORREF(0x00625247));
        if !fill.0.is_null() && !border.0.is_null() {
            let old_fill = SelectObject(hdc, HGDIOBJ(fill.0));
            let old_border = SelectObject(hdc, HGDIOBJ(border.0));
            let _ = RoundRect(hdc, chip.left, chip.top, chip.right, chip.bottom, 16, 16);
            let _ = SelectObject(hdc, old_border);
            let _ = SelectObject(hdc, old_fill);
        }
        if !fill.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(fill.0));
        }
        let accent_brush = CreateSolidBrush(COLORREF(0x00f3c837));
        if !accent_brush.0.is_null() {
            let accent = RECT {
                left: chip.left + 1,
                top: chip.top + 12,
                right: chip.left + 4,
                bottom: chip.bottom - 12,
            };
            let _ = FillRect(hdc, &accent, accent_brush);
            let _ = DeleteObject(HGDIOBJ(accent_brush.0));
        }
        if !border.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(border.0));
        }
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLORREF(0x00ffffff));
        let _ = windows::Win32::Graphics::Gdi::TextOutW(hdc, chip.left + 16, chip.top + 15, hint);
    }
}

fn update_overlay_selection(hwnd: HWND, state: &OverlayState, previous_end: Option<POINT>) {
    let Some(start) = state.start else {
        return;
    };
    let left = start.x.min(state.end.x) - state.desktop.left;
    let top = start.y.min(state.end.y) - state.desktop.top;
    let right = start.x.max(state.end.x) - state.desktop.left;
    let bottom = start.y.max(state.end.y) - state.desktop.top;
    let desktop_width = state.desktop.right - state.desktop.left;
    let desktop_height = state.desktop.bottom - state.desktop.top;
    unsafe {
        let region = CreateRectRgn(0, 0, desktop_width, desktop_height);
        if !region.0.is_null() {
            let width = right - left;
            let height = bottom - top;
            if width >= 2 && height >= 2 {
                let hole = CreateRectRgn(left, top, right, bottom);
                if !hole.0.is_null() {
                    let combined = CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF);
                    let _ = DeleteObject(HGDIOBJ(hole.0));
                    if combined.0 == RGN_ERROR.0 {
                        let _ = DeleteObject(HGDIOBJ(region.0));
                        return;
                    }
                }
            }
            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
            }
        }
    }
    if let Some(previous) = previous_end {
        invalidate_selection_outline(hwnd, state, start, previous);
    }
    invalidate_selection_outline(hwnd, state, start, state.end);
}

fn invalidate_selection_outline(hwnd: HWND, state: &OverlayState, start: POINT, end: POINT) {
    let pad = 3;
    let left = (start.x.min(end.x) - state.desktop.left - pad).max(0);
    let top = (start.y.min(end.y) - state.desktop.top - pad).max(0);
    let right = (start.x.max(end.x) - state.desktop.left + pad)
        .min(state.desktop.right - state.desktop.left);
    let bottom = (start.y.max(end.y) - state.desktop.top + pad)
        .min(state.desktop.bottom - state.desktop.top);
    let rectangles = [
        RECT {
            left,
            top,
            right,
            bottom: (top + pad * 2).min(bottom),
        },
        RECT {
            left,
            top: (bottom - pad * 2).max(top),
            right,
            bottom,
        },
        RECT {
            left,
            top,
            right: (left + pad * 2).min(right),
            bottom,
        },
        RECT {
            left: (right - pad * 2).max(left),
            top,
            right,
            bottom,
        },
    ];
    unsafe {
        for rect in rectangles {
            if rect.right > rect.left && rect.bottom > rect.top {
                let _ = InvalidateRect(Some(hwnd), Some(&rect), false);
            }
        }
    }
}

fn select_and_capture(owner: HWND, spanish: bool) -> Result<(), String> {
    let desktop = RECT {
        left: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN)
        },
        top: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN)
        },
        right: 0,
        bottom: 0,
    };
    let mut desktop = desktop;
    desktop.right = desktop.left
        + unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN) };
    desktop.bottom = desktop.top
        + unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN) };
    if desktop.right <= desktop.left || desktop.bottom <= desktop.top {
        return Err("Windows no informó del tamaño de la pantalla.".into());
    }
    let mut state = OverlayState {
        desktop,
        start: None,
        end: POINT::default(),
        rendered_end: None,
        redraw_timer_active: false,
        cancelled: false,
        hint: if spanish {
            "Arrastra para seleccionar  ·  Esc para cancelar"
                .encode_utf16()
                .collect()
        } else {
            "Drag to select  ·  Esc to cancel".encode_utf16().collect()
        },
    };
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            OVERLAY_CLASS,
            w!("KGen View Screen Capture"),
            WS_POPUP,
            desktop.left,
            desktop.top,
            desktop.right - desktop.left,
            desktop.bottom - desktop.top,
            None,
            None,
            Some(HINSTANCE(module.0)),
            Some((&mut state as *mut OverlayState).cast()),
        )
    }
    .map_err(|error| error.to_string())?;
    unsafe {
        let _ = SetLayeredWindowAttributes(
            hwnd,
            COLORREF(0),
            158,
            windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA,
        );
        let _ = SetWindowPos(
            hwnd,
            Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST),
            desktop.left,
            desktop.top,
            desktop.right - desktop.left,
            desktop.bottom - desktop.top,
            SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
        let _ = UpdateWindow(hwnd);
    }

    let mut message = MSG::default();
    loop {
        let fetched = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if !fetched.as_bool() {
            state.cancelled = true;
            unsafe {
                PostQuitMessage(message.wParam.0 as i32);
            }
            break;
        }
        if message.message == WM_OVERLAY_DONE {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    if state.cancelled {
        return Ok(());
    }
    let Some(start) = state.start else {
        return Ok(());
    };
    let rect = RECT {
        left: start.x.min(state.end.x),
        top: start.y.min(state.end.y),
        right: start.x.max(state.end.x),
        bottom: start.y.max(state.end.y),
    };
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width < 2 || height < 2 {
        return Ok(());
    }
    let mut bgra = capture_bgra(rect)?;
    let destination = pictures_capture_dir()?;
    fs::create_dir_all(&destination)
        .map_err(|error| format!("No se pudo crear {}: {error}", destination.display()))?;
    let path = unique_capture_path(&destination);
    save_png(&path, width as u32, height as u32, &mut bgra)?;
    PreparedImage::new(width as u32, height as u32, &bgra)?.publish(owner)?;
    notify_capture(spanish);
    Ok(())
}

fn capture_bgra(rect: RECT) -> Result<Vec<u8>, String> {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|value| value.checked_mul(4))
        .ok_or("El área seleccionada es demasiado grande.")?;
    if length > crate::resources::CPU_LIMIT / 2 {
        return Err(
            "El área seleccionada supera el límite de memoria de captura (256 MiB).".into(),
        );
    }
    let mut pixels = vec![0u8; length];
    unsafe {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return Err("No se pudo acceder al escritorio para capturar la selección.".into());
        }
        let memory = CreateCompatibleDC(Some(screen));
        if memory.0.is_null() {
            let _ = ReleaseDC(None, screen);
            return Err("No se pudo preparar la captura de pantalla.".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.0.is_null() {
            let _ = DeleteDC(memory);
            let _ = ReleaseDC(None, screen);
            return Err("No se pudo reservar memoria para la captura.".into());
        }
        let old = SelectObject(memory, HGDIOBJ(bitmap.0));
        let blit = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        let _ = SelectObject(memory, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = if blit.is_ok() {
            GetDIBits(
                screen,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast::<c_void>()),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        let _ = ReleaseDC(None, screen);
        if lines != height {
            return Err("No se pudo leer la imagen de la pantalla.".into());
        }
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    Ok(pixels)
}

fn save_png(path: &PathBuf, width: u32, height: u32, bgra: &mut [u8]) -> Result<(), String> {
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let result = (|| {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("No se pudo crear {}: {error}", path.display()))?;
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("No se pudo iniciar PNG: {error}"))?;
        writer
            .write_image_data(bgra)
            .map_err(|error| format!("No se pudo guardar la captura: {error}"))
    })();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    result
}

fn notify_capture(spanish: bool) {
    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
        play_capture_sound();
        return;
    }
    let notification = (|| {
        let (title, body) = if spanish {
            (
                "Captura guardada",
                "Copiada al portapapeles · Imágenes\\KGen",
            )
        } else {
            ("Screenshot saved", "Copied to clipboard · Pictures\\KGen")
        };
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{title}</text><text>{body}</text></binding></visual></toast>"
        );
        let document = XmlDocument::new()?;
        document.LoadXml(&windows::core::HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&document)?;
        let app_id = windows::core::HSTRING::from(APP_USER_MODEL_ID);
        ToastNotificationManager::CreateToastNotifierWithId(&app_id)?.Show(&toast)
    })();
    unsafe { RoUninitialize() };
    play_capture_sound();
    let _ = notification;
}

fn play_capture_sound() {
    unsafe {
        let played = PlaySoundW(
            w!("SystemAsterisk"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
        if !played.as_bool() {
            let _ = MessageBeep(MB_ICONASTERISK);
        }
    }
}

fn pictures_capture_dir() -> Result<PathBuf, String> {
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KNOWN_FOLDER_FLAG(0), None) }
        .map_err(|error| format!("No se pudo encontrar la carpeta Imágenes: {error}"))?;
    if raw.0.is_null() {
        return Err("Windows no devolvió la carpeta Imágenes.".into());
    }
    let mut length = 0usize;
    unsafe {
        while *raw.0.add(length) != 0 {
            length += 1;
        }
    }
    let path = unsafe { std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw.0, length)) };
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    Ok(PathBuf::from(path).join("KGen"))
}

fn unique_capture_path(directory: &std::path::Path) -> PathBuf {
    let utc = unsafe { GetSystemTime() };
    let mut local = Default::default();
    let local = if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_ok() {
        local
    } else {
        utc
    };
    let base = format!(
        "Captura-{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
        local.wYear,
        local.wMonth,
        local.wDay,
        local.wHour,
        local.wMinute,
        local.wSecond,
        local.wMilliseconds
    );
    for suffix in 0..1000u32 {
        let name = if suffix == 0 {
            format!("{base}.png")
        } else {
            format!("{base}-{suffix}.png")
        };
        let path = directory.join(name);
        if !path.exists() {
            return path;
        }
    }
    directory.join(format!("{base}-overflow.png"))
}

fn show_error(message: &str) {
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("KGen View - Captura"),
            windows::Win32::UI::WindowsAndMessaging::MB_OK
                | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

/// Converts hotkey modifier bits from the settings UI to RegisterHotKey flags.
pub fn register_modifiers(modifiers: u32) -> HOT_KEY_MODIFIERS {
    let mut result = 0u32;
    if modifiers & 1 != 0 {
        result |= MOD_ALT.0;
    }
    if modifiers & 2 != 0 {
        result |= MOD_CONTROL.0;
    }
    if modifiers & 4 != 0 {
        result |= MOD_SHIFT.0;
    }
    if modifiers & 8 != 0 {
        result |= MOD_WIN.0;
    }
    HOT_KEY_MODIFIERS(result)
}

pub fn hotkey_text(hotkey: CaptureHotkey, spanish: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if hotkey.modifiers & 2 != 0 {
        parts.push("Ctrl".into());
    }
    if hotkey.modifiers & 1 != 0 {
        parts.push("Alt".into());
    }
    if hotkey.modifiers & 4 != 0 {
        parts.push("Shift".into());
    }
    if hotkey.modifiers & 8 != 0 {
        parts.push("Win".into());
    }
    let key = match hotkey.key {
        0x2c => if spanish { "Impr Pant" } else { "Print Screen" }.to_owned(),
        0x70..=0x87 => format!("F{}", hotkey.key - 0x6f),
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(hotkey.key).unwrap().to_string(),
        _ => format!("VK-{:02X}", hotkey.key),
    };
    parts.push(key);
    parts.join(" + ")
}

pub fn choose_hotkey(
    owner: HWND,
    current: CaptureHotkey,
    spanish: bool,
) -> Result<Option<CaptureHotkey>, String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_HOTKEY_CLASS,
    };
    if !unsafe { InitCommonControlsEx(&controls) }.as_bool() {
        return Err("No se pudo inicializar el control para elegir el atajo.".into());
    }
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let instance = HINSTANCE(module.0);
    register_window_class(
        HOTKEY_DIALOG_CLASS,
        Some(hotkey_dialog_proc),
        instance,
        IDC_ARROW,
        CS_HREDRAW | CS_VREDRAW,
    )?;

    let mut state = Box::new(HotkeyDialogState {
        hotkey_control: HWND::default(),
        windows_key_control: HWND::default(),
        initial: current,
        result: None,
        done: false,
        spanish,
    });
    let state_pointer = &mut *state as *mut HotkeyDialogState;
    let title: Vec<u16> = if spanish {
        "KGen View - Atajo de captura"
            .encode_utf16()
            .chain(Some(0))
            .collect()
    } else {
        "KGen View - Capture shortcut"
            .encode_utf16()
            .chain(Some(0))
            .collect()
    };
    let dialog = unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WS_EX_DLGMODALFRAME
                | windows::Win32::UI::WindowsAndMessaging::WS_EX_CONTROLPARENT,
            HOTKEY_DIALOG_CLASS,
            PCWSTR(title.as_ptr()),
            WS_CAPTION | WS_SYSMENU | WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            390,
            210,
            Some(owner),
            None,
            Some(instance),
            Some(state_pointer.cast()),
        )
    }
    .map_err(|error| error.to_string())?;
    if state.hotkey_control.0.is_null() {
        unsafe {
            let _ = DestroyWindow(dialog);
        }
        return Err("No se pudo crear el control para elegir el atajo.".into());
    }
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(owner, false);
        let _ = ShowWindow(dialog, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(dialog);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(state.hotkey_control));
    }

    let mut message = MSG::default();
    while !state.done && unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        if unsafe { IsDialogMessageW(dialog, &message) }.as_bool() {
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        let _ = DestroyWindow(dialog);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
    }
    Ok(state.result)
}

unsafe extern "system" fn hotkey_dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        let state = unsafe { (*create).lpCreateParams as *mut HotkeyDialogState };
        if !state.is_null() {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        }
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HotkeyDialogState };
    match message {
        windows::Win32::UI::WindowsAndMessaging::WM_CREATE => {
            if state.is_null() {
                return LRESULT(-1);
            }
            let state = unsafe { &mut *state };
            let instance = unsafe { GetModuleHandleW(None) }
                .map(|module| HINSTANCE(module.0))
                .unwrap_or_default();
            let instruction: Vec<u16> = if state.spanish {
                "Elige teclas y usa la casilla Windows:"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect()
            } else {
                "Choose keys; use the Windows checkbox:"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect()
            };
            let _ = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    PCWSTR(instruction.as_ptr()),
                    WS_CHILD | WS_VISIBLE,
                    18,
                    14,
                    345,
                    24,
                    Some(hwnd),
                    None,
                    Some(instance),
                    None,
                )
            };
            state.hotkey_control = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    HOTKEY_CLASS,
                    w!(""),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                    18,
                    42,
                    345,
                    28,
                    Some(hwnd),
                    Some(HMENU(1001usize as *mut c_void)),
                    Some(instance),
                    None,
                )
            }
            .unwrap_or_default();
            let windows_key_text: Vec<u16> = if state.spanish {
                "Tecla Windows".encode_utf16().chain(Some(0)).collect()
            } else {
                "Windows key".encode_utf16().chain(Some(0)).collect()
            };
            state.windows_key_control = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    PCWSTR(windows_key_text.as_ptr()),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_TABSTOP
                        | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                            BS_AUTOCHECKBOX as u32,
                        ),
                    18,
                    78,
                    180,
                    26,
                    Some(hwnd),
                    Some(HMENU(1002usize as *mut c_void)),
                    Some(instance),
                    None,
                )
            }
            .unwrap_or_default();
            if state.initial.modifiers & 8 != 0 {
                let _ = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                        state.windows_key_control,
                        BM_SETCHECK,
                        Some(WPARAM(BST_CHECKED.0 as usize)),
                        Some(LPARAM(0)),
                    )
                };
            }
            let save_text: Vec<u16> = if state.spanish {
                "Guardar".encode_utf16().chain(Some(0)).collect()
            } else {
                "Save".encode_utf16().chain(Some(0)).collect()
            };
            let cancel_text: Vec<u16> = if state.spanish {
                "Cancelar".encode_utf16().chain(Some(0)).collect()
            } else {
                "Cancel".encode_utf16().chain(Some(0)).collect()
            };
            let _ = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    PCWSTR(save_text.as_ptr()),
                    WS_CHILD
                        | WS_VISIBLE
                        | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                            BS_DEFPUSHBUTTON as u32,
                        ),
                    190,
                    122,
                    80,
                    30,
                    Some(hwnd),
                    Some(HMENU(std::ptr::with_exposed_provenance_mut::<c_void>(1))),
                    Some(instance),
                    None,
                )
            };
            let _ = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    PCWSTR(cancel_text.as_ptr()),
                    WS_CHILD | WS_VISIBLE,
                    280,
                    122,
                    83,
                    30,
                    Some(hwnd),
                    Some(HMENU(std::ptr::with_exposed_provenance_mut::<c_void>(2))),
                    Some(instance),
                    None,
                )
            };
            let mut flags = 0u32;
            if state.initial.modifiers & 1 != 0 {
                flags |= HOTKEYF_ALT;
            }
            if state.initial.modifiers & 2 != 0 {
                flags |= HOTKEYF_CONTROL;
            }
            if state.initial.modifiers & 4 != 0 {
                flags |= HOTKEYF_SHIFT;
            }
            let code = (state.initial.key & 0xff) | (flags << 8);
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    state.hotkey_control,
                    HKM_SETHOTKEY,
                    Some(WPARAM(code as usize)),
                    Some(LPARAM(0)),
                )
            };
            LRESULT(0)
        }
        WM_COMMAND => {
            if state.is_null() {
                return LRESULT(0);
            }
            let state = unsafe { &mut *state };
            match wparam.0 & 0xffff {
                1 => {
                    let encoded = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                            state.hotkey_control,
                            HKM_GETHOTKEY,
                            Some(WPARAM(0)),
                            Some(LPARAM(0)),
                        )
                    }
                    .0 as u32;
                    let key = encoded & 0xff;
                    let flags = (encoded >> 8) & 0xff;
                    if key == 0 || key == 0x7b {
                        let text = if state.spanish {
                            "Elige una tecla disponible distinta de F12."
                        } else {
                            "Choose an available key other than F12."
                        };
                        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                        unsafe {
                            let _ = MessageBoxW(
                                Some(hwnd),
                                PCWSTR(text.as_ptr()),
                                w!("KGen View"),
                                windows::Win32::UI::WindowsAndMessaging::MB_OK
                                    | windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
                            );
                        }
                        return LRESULT(0);
                    }
                    let mut modifiers = 0u32;
                    if flags & HOTKEYF_ALT != 0 {
                        modifiers |= 1;
                    }
                    if flags & HOTKEYF_CONTROL != 0 {
                        modifiers |= 2;
                    }
                    if flags & HOTKEYF_SHIFT != 0 {
                        modifiers |= 4;
                    }
                    let windows_checked = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                            state.windows_key_control,
                            BM_GETCHECK,
                            Some(WPARAM(0)),
                            Some(LPARAM(0)),
                        )
                    }
                    .0 == BST_CHECKED.0 as isize;
                    if windows_checked {
                        modifiers |= 8;
                    }
                    state.result = Some(CaptureHotkey { key, modifiers });
                    state.done = true;
                    LRESULT(0)
                }
                2 => {
                    state.done = true;
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            }
        }
        WM_CLOSE => {
            if !state.is_null() {
                unsafe {
                    (*state).done = true;
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
