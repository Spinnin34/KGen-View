use super::*;

pub(super) unsafe fn create_controls(hwnd: HWND, state: &mut DialogState) {
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
