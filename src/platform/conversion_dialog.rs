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

unsafe fn create_controls(hwnd: HWND, state: &mut DialogState) {
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

unsafe fn begin_conversion(hwnd: HWND, state: &mut DialogState) {
    if state.busy {
        return;
    }
    let selected = unsafe {
        SendMessageW(
            state.format_combo,
            CB_GETCURSEL,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
    }
    .0;
    if selected < 0 || selected as usize >= state.formats.len() {
        return;
    }
    let format = state.formats[selected as usize].clone();
    let source_extension = state
        .source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let stem = state
        .source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let suffix = if format.accepts_extension(source_extension) {
        if state.language == Language::Spanish {
            "-convertida"
        } else {
            "-converted"
        }
    } else {
        ""
    };
    let suggested = wide(&format!("{stem}{suffix}.{}", format.extension()));
    let pattern = wide(&format!(
        "{}\0{}\0\0",
        format.filter_label(),
        format.filter_pattern()
    ));
    let extension = wide(format.extension());
    let mut buffer = vec![0u16; 32768];
    let count = suggested.len().min(buffer.len());
    buffer[..count].copy_from_slice(&suggested[..count]);
    let mut dialog = windows::Win32::UI::Controls::Dialogs::OPENFILENAMEW {
        lStructSize: size_of::<windows::Win32::UI::Controls::Dialogs::OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(pattern.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        nFilterIndex: 1,
        Flags: windows::Win32::UI::Controls::Dialogs::OFN_EXPLORER
            | windows::Win32::UI::Controls::Dialogs::OFN_PATHMUSTEXIST
            | windows::Win32::UI::Controls::Dialogs::OFN_OVERWRITEPROMPT,
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        ..Default::default()
    };
    if !unsafe { windows::Win32::UI::Controls::Dialogs::GetSaveFileNameW(&mut dialog) }.as_bool() {
        return;
    }
    let mut path = PathBuf::from(String::from_utf16_lossy(
        &buffer[..buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len())],
    ));
    let path_extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !format.accepts_extension(path_extension) {
        path.set_extension(format.extension());
    }

    state.busy = true;
    state.dots = 0;
    unsafe {
        let _ = EnableWindow(state.format_combo, false);
        let _ = EnableWindow(state.format_arrow_button, false);
        let _ = EnableWindow(state.convert_button, false);
        let _ =
            windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), TIMER_DOTS, 320, None);
        let status = if state.language == Language::Spanish {
            "Convirtiendo…"
        } else {
            "Converting…"
        };
        let text = wide(status);
        let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
    }
    let snapshot = state.snapshot.clone();
    let completion = state.completion.clone();
    let hwnd_value = hwnd.0 as usize;
    let language = state.language;
    let worker = thread::Builder::new()
        .name("KGen image conversion".into())
        .spawn(move || {
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let result = if initialized {
                snapshot
                    .export_copy_as(&path, &format)
                    .map(|()| path)
                    .map_err(|error| error.to_string())
            } else {
                Err(if language == Language::Spanish {
                    "No se pudo inicializar Windows Imaging Component en segundo plano.".into()
                } else {
                    "Windows Imaging Component could not be initialized on the background thread."
                        .into()
                })
            };
            if initialized {
                unsafe { CoUninitialize() };
            }
            if let Ok(mut slot) = completion.lock() {
                *slot = Some(result);
            }
            let target = HWND(hwnd_value as *mut c_void);
            unsafe {
                let _ = PostMessageW(Some(target), WM_CONVERSION_DONE, WPARAM(0), LPARAM(0));
            }
        });
    if let Err(error) = worker {
        state.busy = false;
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_DOTS);
            let _ = EnableWindow(state.format_combo, true);
            let _ = EnableWindow(state.format_arrow_button, true);
            let _ = EnableWindow(state.convert_button, true);
            let message = if state.language == Language::Spanish {
                format!("No se pudo iniciar la conversión: {error}")
            } else {
                format!("Could not start conversion: {error}")
            };
            let text = wide(&message);
            let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
        }
    }
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

unsafe fn draw_item(item: &DRAWITEMSTRUCT, state: &DialogState) {
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
