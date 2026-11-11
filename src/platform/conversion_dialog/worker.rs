use super::*;

pub(super) unsafe fn begin_conversion(hwnd: HWND, state: &mut DialogState) {
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
