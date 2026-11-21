use std::{
    cmp::Reverse,
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Foundation::HWND,
        Globalization::{CSTR_EQUAL, CompareStringOrdinal},
        System::{
            Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance, CoTaskMemFree, IServiceProvider},
            Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4},
        },
        UI::{
            Shell::{
                IFolderView, IPersistFolder2, IShellBrowser, IShellItemArray, IShellWindows,
                IWebBrowser2, SHGetNameFromIDList, SID_STopLevelBrowser, SIGDN_FILESYSPATH,
                SVGIO_ALLVIEW, SVGIO_FLAG_VIEWORDER, ShellWindows,
            },
            WindowsAndMessaging::{GetClassNameW, IsWindowVisible},
        },
    },
    core::{Interface, PWSTR, w},
};

pub fn folder_windows() -> windows::core::Result<impl Iterator<Item = IWebBrowser2>> {
    let windows: IShellWindows =
        unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)? };
    let count = unsafe { windows.Count()? };
    Ok((0..count).filter_map(move |index| {
        let dispatch = unsafe { windows.Item(&integer_variant(index)).ok()? };
        dispatch.cast().ok()
    }))
}

pub fn is_explorer_window(hwnd: HWND) -> bool {
    let mut class = [0; 32];
    let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    let class = &class[..len];
    unsafe { class == w!("CabinetWClass").as_wide() || class == w!("ExploreWClass").as_wide() }
}

pub fn folder_file_names(
    folder: &Path,
    preferred_window: usize,
    should_cancel: &impl Fn() -> bool,
) -> Option<Vec<OsString>> {
    let folder = comparable_path(folder);
    let mut best = None;
    for browser in folder_windows().ok()? {
        if should_cancel() {
            return None;
        }
        let Some((view, window, view_window)) = folder_view(&browser, &folder) else {
            continue;
        };
        // Explorer tabs can share a top-level HWND. Prefer its displayed tab,
        // and use window handles to break ties independently of COM enumeration.
        let visible = unsafe { IsWindowVisible(view_window).as_bool() };
        let preferred = window == preferred_window;
        let rank = (
            visible && preferred,
            visible,
            preferred,
            Reverse(window),
            Reverse(view_window.0 as usize),
        );
        if best.as_ref().is_none_or(|(best_rank, _)| rank > *best_rank) {
            best = Some((rank, view));
        }
    }

    let (_, view) = best?;
    // Without VIEWORDER the focused item can be returned first.
    let items: IShellItemArray = unsafe { view.Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER).ok()? };
    let mut names = Vec::new();
    for index in 0..unsafe { items.GetCount().ok()? } {
        if should_cancel() {
            return None;
        }
        let item = unsafe { items.GetItemAt(index).ok()? };
        if let Ok(path) = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) } {
            let path = take_shell_path(path);
            if let Some(name) = path.file_name() {
                names.push(name.to_os_string());
            }
        }
    }
    Some(names)
}

fn folder_view(browser: &IWebBrowser2, folder: &[u16]) -> Option<(IFolderView, usize, HWND)> {
    let window = unsafe { browser.HWND().ok()?.0 } as usize;
    let services = browser.cast::<IServiceProvider>().ok()?;
    let shell: IShellBrowser = unsafe { services.QueryService(&SID_STopLevelBrowser).ok()? };
    let shell_view = unsafe { shell.QueryActiveShellView().ok()? };
    let view = shell_view.cast::<IFolderView>().ok()?;
    let persist: IPersistFolder2 = unsafe { view.GetFolder().ok()? };
    let pidl = unsafe { persist.GetCurFolder().ok()? };
    let location = unsafe { SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH) };
    unsafe { CoTaskMemFree(Some(pidl.cast())) };
    let path = comparable_path(&take_shell_path(location.ok()?));
    if unsafe { CompareStringOrdinal(folder, &path, true) } != CSTR_EQUAL {
        return None;
    }
    Some((view, window, unsafe { shell_view.GetWindow().ok()? }))
}

fn comparable_path(path: &Path) -> Vec<u16> {
    let path: Vec<_> = path.as_os_str().encode_wide().collect();
    // Rust canonicalizes local and UNC paths with a verbatim prefix, which
    // Explorer normally omits.
    unsafe {
        if let Some(unc) = path.strip_prefix(w!(r"\\?\UNC\").as_wide()) {
            [w!(r"\\").as_wide(), unc].concat()
        } else if let Some(local) = path.strip_prefix(w!(r"\\?\").as_wide()) {
            local.to_vec()
        } else {
            path
        }
    }
}

fn take_shell_path(path: PWSTR) -> PathBuf {
    unsafe {
        let value = OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        PathBuf::from(value)
    }
}

fn integer_variant(value: i32) -> VARIANT {
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
