use super::*;

pub(super) fn metadata_value(document: &ImageDocument, label: &str) -> String {
    document
        .metadata
        .iter()
        .find(|(candidate, _)| candidate == label)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

pub(super) fn document_bytes(document: &ImageDocument) -> usize {
    document.frames.iter().map(|frame| frame.byte_len()).sum()
}

pub(super) fn set_metadata_value(document: &mut ImageDocument, label: &str, value: &str) -> bool {
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

pub(super) fn file_timestamp(path: &Path) -> String {
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

pub(super) fn file_timestamp_short(path: &Path) -> String {
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

pub(super) fn folder_modified(path: &Path) -> Option<SystemTime> {
    path.parent().and_then(|folder| {
        fs::metadata(folder)
            .and_then(|metadata| metadata.modified())
            .ok()
    })
}

pub(super) fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        value.into_owned()
    }
}

pub(super) fn command_line_paths() -> Vec<PathBuf> {
    let mut arguments = std::env::args_os().skip(1);
    let mut paths = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--mcp" {
            continue;
        }
        if argument == "--open-archive-member" {
            arguments.next();
            arguments.next();
            continue;
        }
        let path = PathBuf::from(argument);
        if path.exists() {
            paths.push(path);
        }
    }
    paths
}

pub(super) fn restored_archive_member() -> Option<ExplorerArchiveSelection> {
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument != "--open-archive-member" {
            continue;
        }
        let archive_path = PathBuf::from(arguments.next()?);
        let member_name = arguments.next()?.into_string().ok()?;
        if !archive::is_archive_path(&archive_path)
            || !archive_path.is_file()
            || member_name.is_empty()
        {
            return None;
        }
        return Some(ExplorerArchiveSelection {
            archive_path,
            member_name,
        });
    }
    None
}

struct ExplorerArchiveContext {
    archive_path: PathBuf,
    member_folder: String,
    focused_item: Option<String>,
    foreground: bool,
}

pub(super) struct ExplorerArchiveSelection {
    pub(super) archive_path: PathBuf,
    pub(super) member_name: String,
}

pub(super) fn resolve_explorer_archive_selection(
    opened_path: &Path,
    launch_window: HWND,
) -> Option<ExplorerArchiveSelection> {
    let opened_path = opened_path.canonicalize().ok()?;
    let temp_directory = std::env::temp_dir().canonicalize().ok()?;
    let (archive_name, member_name) =
        parse_explorer_archive_member_path(&opened_path, &temp_directory)?;
    let expected_folder = Path::new(&member_name)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .replace('\\', "/");
    let expected_file_name = Path::new(&member_name)
        .file_name()?
        .to_string_lossy()
        .into_owned();

    let mut best_match: Option<(PathBuf, i32)> = None;
    let mut ambiguous = false;
    for context in shell_archive_contexts(launch_window) {
        let Some(source_name) = context
            .archive_path
            .file_name()
            .and_then(|name| name.to_str())
        else {
            continue;
        };
        if !source_name.eq_ignore_ascii_case(&archive_name) {
            continue;
        }
        let mut score = 0;
        if context.foreground {
            score += 16;
        }
        if context.member_folder.eq_ignore_ascii_case(&expected_folder) {
            score += 8;
        }
        if context
            .focused_item
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&expected_file_name))
        {
            score += 4;
        }
        match &best_match {
            Some((best_path, best_score)) if *best_score > score => {}
            Some((best_path, best_score)) if *best_score == score => {
                if best_path != &context.archive_path {
                    ambiguous = true;
                }
            }
            _ => {
                best_match = Some((context.archive_path, score));
                ambiguous = false;
            }
        }
    }
    if ambiguous {
        return None;
    }
    Some(ExplorerArchiveSelection {
        archive_path: best_match?.0,
        member_name,
    })
}

fn parse_explorer_archive_member_path(
    opened_path: &Path,
    temp_directory: &Path,
) -> Option<(String, String)> {
    let relative_path = opened_path.strip_prefix(temp_directory).ok()?;
    let mut components = relative_path.components();
    let temp_folder_name = components.next()?.as_os_str().to_str()?;
    let archive_name = archive_name_from_explorer_temp_folder(temp_folder_name)?.to_owned();
    let member_path = components.as_path();
    if member_path.as_os_str().is_empty() || !crate::catalog::is_supported(member_path) {
        return None;
    }
    let member_name = member_path.to_string_lossy().replace('\\', "/");
    Some((archive_name, member_name))
}

pub(super) fn archive_name_from_explorer_temp_folder(folder_name: &str) -> Option<&str> {
    let after_prefix = folder_name
        .get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case("Temp"))
        .and_then(|_| folder_name.get(4..))?;
    let digit_count = after_prefix
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digit_count == 0 {
        return None;
    }
    let suffix = after_prefix.get(digit_count..)?.strip_prefix('_')?;
    archive::is_archive_path(Path::new(suffix)).then_some(suffix)
}

fn shell_archive_contexts(launch_window: HWND) -> Vec<ExplorerArchiveContext> {
    let Ok(shell_windows) =
        (unsafe { CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_LOCAL_SERVER) })
    else {
        return Vec::new();
    };
    let Ok(window_count) = (unsafe { shell_windows.Count() }) else {
        return Vec::new();
    };
    let mut contexts = Vec::new();
    for index in 0..window_count {
        let index_variant = integer_variant(index);
        let Ok(dispatch) = (unsafe { shell_windows.Item(&index_variant) }) else {
            continue;
        };
        let Ok(browser) = dispatch.cast::<IWebBrowser2>() else {
            continue;
        };
        let view = unsafe { browser.Document() }
            .ok()
            .and_then(|document| document.cast::<IShellFolderViewDual>().ok());
        let folder_path = view
            .as_ref()
            .and_then(|view| unsafe { view.Folder().ok() })
            .and_then(|folder| folder.cast::<Folder2>().ok())
            .and_then(|folder| unsafe { folder.Self_().ok() })
            .and_then(|item| unsafe { item.Path().ok() })
            .and_then(|path| String::try_from(path).ok())
            .map(PathBuf::from);
        // Modern Explorer tabs may not expose IShellFolderViewDual, or may
        // return a virtual namespace path. LocationURL can still identify the
        // compressed folder in either case.
        let location_path = unsafe { browser.LocationURL().ok() }
            .and_then(|location| String::try_from(location).ok())
            .and_then(|location| file_url_to_path(&location));
        let location = folder_path
            .and_then(|path| archive_path_for_shell_location(&path).map(|archive| (path, archive)))
            .or_else(|| {
                location_path.and_then(|path| {
                    archive_path_for_shell_location(&path).map(|archive| (path, archive))
                })
            });
        let Some((current_path, archive_path)) = location else {
            continue;
        };
        let member_folder = current_path
            .strip_prefix(&archive_path)
            .unwrap_or_else(|_| Path::new(""))
            .to_string_lossy()
            .replace('\\', "/");
        let focused_item = view
            .as_ref()
            .and_then(|view| unsafe { view.FocusedItem().ok() })
            .and_then(|item| unsafe { item.Name().ok() })
            .and_then(|name| String::try_from(name).ok());
        let foreground = unsafe { browser.HWND() }
            .ok()
            .is_some_and(|window| window.0 == launch_window.0 as isize);
        contexts.push(ExplorerArchiveContext {
            archive_path,
            member_folder,
            focused_item,
            foreground,
        });
    }
    contexts
}

fn archive_path_for_shell_location(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| archive::is_archive_path(ancestor) && ancestor.is_file())
        .map(Path::to_path_buf)
}

fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let (encoded_path, unc) = if url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:///"))
    {
        (&url[8..], false)
    } else if url
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
    {
        (&url[7..], true)
    } else {
        return None;
    };

    let mut decoded = Vec::with_capacity(encoded_path.len());
    let bytes = encoded_path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    let windows_path = if unc {
        format!("\\\\{}", decoded.replace('/', "\\"))
    } else {
        decoded.replace('/', "\\")
    };
    Some(PathBuf::from(windows_path))
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn integer_variant(value: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: value },
            }),
        },
    }
}

pub(super) unsafe fn state_mut<'a>(hwnd: HWND) -> &'a mut AppState {
    unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState) }
}

pub(super) fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}

pub(super) fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}

pub(super) fn point_from_lparam(value: LPARAM) -> POINT {
    POINT {
        x: loword(value.0 as usize) as i16 as i32,
        y: hiword(value.0 as usize) as i16 as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_temp_path_resolves_the_nested_archive_member() {
        let temp = std::env::temp_dir();
        let opened = temp
            .join("Temp12_Comic.CBZ")
            .join("Chapter 1")
            .join("page02.PNG");
        assert_eq!(
            parse_explorer_archive_member_path(&opened, &temp),
            Some(("Comic.CBZ".into(), "Chapter 1/page02.PNG".into()))
        );
    }

    #[test]
    fn explorer_temp_path_rejects_unrelated_or_unsupported_files() {
        let temp = std::env::temp_dir();
        assert!(
            parse_explorer_archive_member_path(&temp.join("photos/page02.png"), &temp).is_none()
        );
        assert!(
            parse_explorer_archive_member_path(&temp.join("Temp12_Comic.zip/readme.txt"), &temp)
                .is_none()
        );
    }

    #[test]
    fn explorer_file_location_urls_resolve_zip_paths() {
        assert_eq!(
            file_url_to_path("file:///C:/Users/Ahmed/Comic%20Book.zip"),
            Some(PathBuf::from(r"C:\Users\Ahmed\Comic Book.zip"))
        );
        assert_eq!(
            file_url_to_path("file://server/share/Comic%20Book.zip"),
            Some(PathBuf::from(r"\\server\share\Comic Book.zip"))
        );
        assert!(file_url_to_path("https://example.com/Comic.zip").is_none());
        assert!(file_url_to_path("file:///C:/Comic%XZ.zip").is_none());
    }
}
