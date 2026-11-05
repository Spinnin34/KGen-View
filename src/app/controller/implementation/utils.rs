fn metadata_value(document: &ImageDocument, label: &str) -> String {
    document
        .metadata
        .iter()
        .find(|(candidate, _)| candidate == label)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn document_bytes(document: &ImageDocument) -> usize {
    document.frames.iter().map(|frame| frame.pixels.len()).sum()
}

fn set_metadata_value(document: &mut ImageDocument, label: &str, value: &str) -> bool {
    if let Some(index) = document
        .metadata
        .iter()
        .position(|(candidate, _)| candidate == label)
    {
        if document.metadata[index].1 == value {
            return false;
        }
        if value.is_empty() {
            document.metadata.remove(index);
        } else {
            document.metadata[index].1 = value.into();
        }
        true
    } else if value.is_empty() {
        false
    } else {
        document.metadata.push((label.into(), value.into()));
        true
    }
}

fn file_timestamp(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return "—".into();
    };
    let raw = metadata.last_write_time();
    let file_time = FILETIME {
        dwLowDateTime: raw as u32,
        dwHighDateTime: (raw >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.is_err()
        || unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err()
    {
        return "—".into();
    }
    format!(
        "{:02}/{:02}/{:04} · {:02}:{:02}:{:02}",
        local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute, local.wSecond
    )
}

fn file_timestamp_short(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return "—".into();
    };
    let raw = metadata.last_write_time();
    let file_time = FILETIME {
        dwLowDateTime: raw as u32,
        dwHighDateTime: (raw >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.is_err()
        || unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err()
    {
        return "—".into();
    }
    format!(
        "{:02}/{:02}/{:04} / {:02}:{:02}:{:02}",
        local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute, local.wSecond
    )
}

fn folder_modified(path: &Path) -> Option<SystemTime> {
    path.parent().and_then(|folder| {
        fs::metadata(folder)
            .and_then(|metadata| metadata.modified())
            .ok()
    })
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        value.into_owned()
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn command_line_paths() -> Vec<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

unsafe fn state_mut<'a>(hwnd: HWND) -> &'a mut AppState {
    unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState) }
}

fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}

fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}

fn point_from_lparam(value: LPARAM) -> POINT {
    POINT {
        x: loword(value.0 as usize) as i16 as i32,
        y: hiword(value.0 as usize) as i16 as i32,
    }
}

