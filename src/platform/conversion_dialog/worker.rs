use super::*;

pub(super) fn begin_conversion(hwnd: HWND, state: &DialogState) {
    if state.busy.get() {
        return;
    }
    let selected =
        unsafe { SendMessageW(state.controls.get().formats, LB_GETCURSEL, None, None) }.0;
    let Some(format) = state.formats.get(selected as usize).cloned() else {
        return;
    };
    let source = &state.snapshot.path;
    let source_extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let suffix = if format.accepts_extension(source_extension) {
        state.label("-convertida", "-converted")
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

    if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } != std::ptr::from_ref(state) as isize {
        return;
    }
    set_busy(hwnd, state, true);
    state.dots.set(0);
    state.set_status(
        state.label("Convirtiendo…", "Converting…"),
        state.palette.muted,
    );
    unsafe {
        let _ = SetTimer(Some(hwnd), TIMER_CONVERSION, 320, None);
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
            let target = HWND(hwnd_value as *mut _);
            unsafe {
                let _ = PostMessageW(Some(target), WM_CONVERSION_DONE, WPARAM(0), LPARAM(0));
            }
        });
    if let Err(error) = worker {
        finish_conversion(
            hwnd,
            state,
            Err(format!(
                "{} {error}",
                state.label(
                    "No se pudo iniciar la conversión.",
                    "Could not start the conversion."
                )
            )),
        );
    }
}

pub(super) fn conversion_tick(hwnd: HWND, state: &DialogState, animate: bool) {
    if !state.busy.get() {
        return;
    }
    let result = state
        .completion
        .try_lock()
        .ok()
        .and_then(|mut result| result.take());
    if let Some(result) = result {
        finish_conversion(hwnd, state, result);
    } else if animate {
        let dots = (state.dots.get() + 1) % 3;
        state.dots.set(dots);
        let label = state.label("Convirtiendo", "Converting");
        state.set_status(
            &format!("{label}{}", ".".repeat(dots + 1)),
            state.palette.muted,
        );
    }
}

fn finish_conversion(hwnd: HWND, state: &DialogState, result: Result<PathBuf, String>) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_CONVERSION);
    }
    set_busy(hwnd, state, false);
    match result {
        Ok(path) => {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let label = state.label("Guardado como", "Saved as");
            state.set_status(&format!("{label} {name}"), state.palette.text);
        }
        Err(error) => state.set_status(&error, if state.dark { 0xffaca3 } else { 0xa02921 }),
    }
}

fn set_busy(hwnd: HWND, state: &DialogState, busy: bool) {
    state.busy.set(busy);
    let controls = state.controls.get();
    let focus = unsafe { GetFocus() };
    if busy && (focus == controls.formats || focus == controls.convert) {
        unsafe {
            let _ = SetFocus(Some(controls.close));
        }
    }
    for control in [controls.formats, controls.convert] {
        unsafe {
            let _ = EnableWindow(control, !busy);
            let _ = InvalidateRect(Some(control), None, false);
        }
    }
    if !busy && unsafe { GetActiveWindow() } == hwnd && (focus.0.is_null() || focus == hwnd) {
        unsafe {
            let _ = SetFocus(Some(controls.formats));
        }
    }
}
