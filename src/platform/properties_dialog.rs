use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            CreateSolidBrush, DEFAULT_GUI_FONT, DeleteObject, FillRect, GetStockObject, HBRUSH,
            HGDIOBJ, SetBkColor, SetTextColor,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::SetWindowTheme,
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW,
                DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
                ES_MULTILINE, ES_READONLY, ES_WANTRETURN, GWLP_USERDATA, GetClientRect,
                GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow, LoadCursorW, MSG,
                PostQuitMessage, RegisterClassExW, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
                WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_NCCREATE,
                WM_SETFONT, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    i18n::Language,
    platform::{text::wide, theme::apply_window_theme},
};

const CLASS_NAME: PCWSTR = w!("KGenView.PropertiesWindow");
const ID_SAVE: usize = 1;
const ID_CANCEL: usize = 2;

pub struct PropertiesData {
    pub file_name: String,
    pub folder: String,
    pub full_path: String,
    pub format: String,
    pub resolution: String,
    pub original_size: String,
    pub current_size: String,
    pub print_size: String,
    pub current_colors: String,
    pub unique_colors: String,
    pub disk_size: String,
    pub memory_size: String,
    pub folder_index: String,
    pub file_time: String,
    pub load_time: String,
    pub author: String,
    pub description: String,
    pub api_key: String,
}

pub struct PropertiesEdit {
    pub file_name: String,
    pub author: String,
    pub description: String,
    pub api_key: String,
}

struct DialogState {
    data: PropertiesData,
    dark: bool,
    language: Language,
    result: Option<PropertiesEdit>,
    name_edit: HWND,
    author_edit: HWND,
    description_edit: HWND,
    api_key_edit: HWND,
    bg_brush: HBRUSH,
    edit_brush: HBRUSH,
    readonly_brush: HBRUSH,
}

pub fn show(
    owner: HWND,
    dark: bool,
    language: Language,
    data: PropertiesData,
) -> Option<PropertiesEdit> {
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.ok()?.0);
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.ok()?;
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&class);
    }

    let bg_color = if dark {
        COLORREF(0x001B1B1B)
    } else {
        COLORREF(0x00F8F8F8)
    };
    let edit_color = if dark {
        COLORREF(0x002B2B2B)
    } else {
        COLORREF(0x00FFFFFF)
    };
    let readonly_color = if dark {
        COLORREF(0x00202020)
    } else {
        COLORREF(0x00EFEFEF)
    };

    let bg_brush = unsafe { CreateSolidBrush(bg_color) };
    let edit_brush = unsafe { CreateSolidBrush(edit_color) };
    let readonly_brush = unsafe { CreateSolidBrush(readonly_color) };

    let state = Box::new(DialogState {
        data,
        dark,
        language,
        result: None,
        name_edit: HWND::default(),
        author_edit: HWND::default(),
        description_edit: HWND::default(),
        api_key_edit: HWND::default(),
        bg_brush,
        edit_brush,
        readonly_brush,
    });
    let pointer = Box::into_raw(state);
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let width = (500.0 * scale).round() as i32;
    let height = (620.0 * scale).round() as i32;
    let title = wide(if language == Language::Spanish {
        "KGen View — Propiedades de la imagen"
    } else {
        "KGen View — Image properties"
    });
    let hwnd = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
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
            unsafe {
                let state = Box::from_raw(pointer);
                let _ = DeleteObject(HGDIOBJ(state.bg_brush.0));
                let _ = DeleteObject(HGDIOBJ(state.edit_brush.0));
                let _ = DeleteObject(HGDIOBJ(state.readonly_brush.0));
            }
            return None;
        }
    };

    unsafe { apply_window_theme(hwnd, dark) };
    center_on_owner(hwnd, owner, width, height);

    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    let mut message = MSG::default();
    while unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        let get_message = unsafe { GetMessageW(&mut message, Some(hwnd), 0, 0) };
        if get_message.0 <= 0 {
            if message.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
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
        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
    }
    let state = unsafe { Box::from_raw(pointer) };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(state.bg_brush.0));
        let _ = DeleteObject(HGDIOBJ(state.edit_brush.0));
        let _ = DeleteObject(HGDIOBJ(state.readonly_brush.0));
    }
    state.result
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState };
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_CREATE => {
            unsafe { create_controls(hwnd, state) };
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                ID_SAVE => {
                    state.result = Some(PropertiesEdit {
                        file_name: window_text(state.name_edit),
                        author: window_text(state.author_edit),
                        description: window_text(state.description_edit),
                        api_key: window_text(state.api_key_edit),
                    });
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                ID_CANCEL => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            let control_hwnd = HWND(lparam.0 as *mut c_void);
            let is_readonly_edit = unsafe {
                let style = GetWindowLongPtrW(
                    control_hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                ) as u32;
                (style as i32) & ES_READONLY != 0
            };
            if is_readonly_edit {
                unsafe {
                    SetTextColor(
                        hdc,
                        if state.dark {
                            COLORREF(0x00E0E0E0)
                        } else {
                            COLORREF(0x001B1B1B)
                        },
                    );
                    SetBkColor(
                        hdc,
                        if state.dark {
                            COLORREF(0x00202020)
                        } else {
                            COLORREF(0x00EFEFEF)
                        },
                    );
                }
                LRESULT(state.readonly_brush.0 as isize)
            } else {
                unsafe {
                    SetTextColor(
                        hdc,
                        if state.dark {
                            COLORREF(0x00F0F0F0)
                        } else {
                            COLORREF(0x001B1B1B)
                        },
                    );
                    SetBkColor(
                        hdc,
                        if state.dark {
                            COLORREF(0x001B1B1B)
                        } else {
                            COLORREF(0x00F8F8F8)
                        },
                    );
                }
                LRESULT(state.bg_brush.0 as isize)
            }
        }
        WM_CTLCOLOREDIT => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            unsafe {
                SetTextColor(
                    hdc,
                    if state.dark {
                        COLORREF(0x00F0F0F0)
                    } else {
                        COLORREF(0x00111111)
                    },
                );
                SetBkColor(
                    hdc,
                    if state.dark {
                        COLORREF(0x002B2B2B)
                    } else {
                        COLORREF(0x00FFFFFF)
                    },
                );
            }
            LRESULT(state.edit_brush.0 as isize)
        }
        WM_CTLCOLORBTN => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            unsafe {
                SetTextColor(
                    hdc,
                    if state.dark {
                        COLORREF(0x00F0F0F0)
                    } else {
                        COLORREF(0x001B1B1B)
                    },
                );
                SetBkColor(
                    hdc,
                    if state.dark {
                        COLORREF(0x001B1B1B)
                    } else {
                        COLORREF(0x00F8F8F8)
                    },
                );
            }
            LRESULT(state.bg_brush.0 as isize)
        }
        WM_ERASEBKGND => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void);
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                FillRect(hdc, &rect, state.bg_brush);
            }
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn create_controls(hwnd: HWND, state: &mut DialogState) {
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let theme_name = if state.dark {
        w!("DarkMode_Explorer")
    } else {
        w!("Explorer")
    };

    let spanish = state.language == Language::Spanish;
    let rows = [
        (
            if spanish { "Carpeta" } else { "Folder" },
            state.data.folder.as_str(),
            true,
        ),
        (
            if spanish {
                "Ruta completa"
            } else {
                "Full path"
            },
            state.data.full_path.as_str(),
            true,
        ),
        (
            if spanish { "Formato" } else { "Format" },
            state.data.format.as_str(),
            false,
        ),
        (
            if spanish { "Resolución" } else { "Resolution" },
            state.data.resolution.as_str(),
            false,
        ),
        (
            if spanish {
                "Tamaño original"
            } else {
                "Original size"
            },
            state.data.original_size.as_str(),
            false,
        ),
        (
            if spanish {
                "Tamaño actual"
            } else {
                "Current size"
            },
            state.data.current_size.as_str(),
            false,
        ),
        (
            if spanish {
                "Tamaño impresión"
            } else {
                "Print size"
            },
            state.data.print_size.as_str(),
            false,
        ),
        (
            if spanish {
                "Formato de píxel"
            } else {
                "Pixel format"
            },
            state.data.current_colors.as_str(),
            false,
        ),
        (
            if spanish {
                "Nº colores únicos"
            } else {
                "Unique colors"
            },
            state.data.unique_colors.as_str(),
            false,
        ),
        (
            if spanish {
                "Tamaño en disco"
            } else {
                "Disk size"
            },
            state.data.disk_size.as_str(),
            false,
        ),
        (
            if spanish {
                "Memoria decodificada"
            } else {
                "Decoded memory"
            },
            state.data.memory_size.as_str(),
            false,
        ),
        (
            if spanish {
                "Índice carpeta/lista"
            } else {
                "Folder position"
            },
            state.data.folder_index.as_str(),
            false,
        ),
        (
            if spanish {
                "Fecha/Hora archivo"
            } else {
                "File date/time"
            },
            state.data.file_time.as_str(),
            false,
        ),
        (
            if spanish { "Cargado en" } else { "Loaded in" },
            state.data.load_time.as_str(),
            false,
        ),
    ];

    let mut y = 10;
    create_static(
        hwnd,
        if spanish {
            "Nombre de archivo"
        } else {
            "File name"
        },
        10,
        y + 2,
        125,
        18,
        scale,
    );
    state.name_edit = create_edit(
        hwnd,
        &state.data.file_name,
        140,
        y,
        345,
        20,
        false,
        false,
        scale,
    );
    let _ = unsafe { SetWindowTheme(state.name_edit, theme_name, None) };
    y += 24;

    for (label, value, long_value) in rows {
        create_static(hwnd, label, 10, y + 2, 125, 18, scale);
        let height = if long_value { 42 } else { 20 };
        let edit = create_edit(hwnd, value, 140, y, 345, height, true, long_value, scale);
        let _ = unsafe { SetWindowTheme(edit, theme_name, None) };
        y += if long_value { 45 } else { 23 };
    }

    create_static(
        hwnd,
        if spanish { "Autor" } else { "Author" },
        10,
        y + 2,
        125,
        18,
        scale,
    );
    state.author_edit = create_edit(
        hwnd,
        &state.data.author,
        140,
        y,
        345,
        20,
        false,
        false,
        scale,
    );
    let _ = unsafe { SetWindowTheme(state.author_edit, theme_name, None) };
    y += 24;

    create_static(
        hwnd,
        if spanish {
            "Descripción"
        } else {
            "Description"
        },
        10,
        y + 2,
        125,
        18,
        scale,
    );
    state.description_edit = create_edit(
        hwnd,
        &state.data.description,
        140,
        y,
        345,
        42,
        false,
        true,
        scale,
    );
    let _ = unsafe { SetWindowTheme(state.description_edit, theme_name, None) };
    y += 46;

    create_static(
        hwnd,
        if spanish {
            "Clave API ImgBB"
        } else {
            "ImgBB API key"
        },
        10,
        y + 2,
        125,
        18,
        scale,
    );
    state.api_key_edit = create_edit(
        hwnd,
        &state.data.api_key,
        140,
        y,
        345,
        20,
        false,
        false,
        scale,
    );
    let _ = unsafe { SetWindowTheme(state.api_key_edit, theme_name, None) };
    y += 30;

    let cancel_btn = create_button(
        hwnd,
        if spanish { "Cancelar" } else { "Cancel" },
        ID_CANCEL,
        280,
        y,
        95,
        26,
        scale,
    );
    let save_btn = create_button(
        hwnd,
        if spanish { "Guardar" } else { "Save" },
        ID_SAVE,
        390,
        y,
        95,
        26,
        scale,
    );
    let _ = unsafe { SetWindowTheme(cancel_btn, theme_name, None) };
    let _ = unsafe { SetWindowTheme(save_btn, theme_name, None) };
    if [
        state.name_edit,
        state.author_edit,
        state.description_edit,
        state.api_key_edit,
        cancel_btn,
        save_btn,
    ]
    .into_iter()
    .any(|control| !unsafe { IsWindow(Some(control)) }.as_bool())
    {
        let _ = unsafe { EnableWindow(save_btn, false) };
    }
}

fn create_static(hwnd: HWND, text: &str, x: i32, y: i32, wdt: i32, hgt: i32, scale: f32) {
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            scaled(x, scale),
            scaled(y, scale),
            scaled(wdt, scale),
            scaled(hgt, scale),
            Some(hwnd),
            None,
            None,
            None,
        )
    };
    if let Ok(control) = control {
        set_font(control);
    }
}

#[allow(clippy::too_many_arguments)]
fn create_edit(
    hwnd: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    read_only: bool,
    multiline: bool,
    scale: f32,
) -> HWND {
    let text = wide(text);
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
    if multiline {
        style |= WINDOW_STYLE(ES_MULTILINE as u32)
            | WINDOW_STYLE(ES_AUTOVSCROLL as u32)
            | WINDOW_STYLE(ES_WANTRETURN as u32)
            | WS_VSCROLL;
    } else {
        style |= WINDOW_STYLE(ES_AUTOHSCROLL as u32);
    }
    if read_only {
        style |= WINDOW_STYLE(ES_READONLY as u32);
    } else {
        style |= WS_BORDER;
    }
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR(text.as_ptr()),
            style,
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
    set_font(control);
    control
}

#[allow(clippy::too_many_arguments)]
fn create_button(
    hwnd: HWND,
    text: &str,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
) -> HWND {
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
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
    set_font(control);
    control
}

fn set_font(hwnd: HWND) {
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

fn window_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let read = unsafe { GetWindowTextW(hwnd, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..read])
}

fn center_on_owner(hwnd: HWND, owner: HWND, width: i32, height: i32) {
    let mut owner_rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &mut owner_rect);
        let x = owner_rect.left + ((owner_rect.right - owner_rect.left - width) / 2).max(0);
        let y = owner_rect.top + ((owner_rect.bottom - owner_rect.top - height) / 2).max(0);
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

fn scaled(value: i32, scale: f32) -> i32 {
    (value as f32 * scale).round() as i32
}
