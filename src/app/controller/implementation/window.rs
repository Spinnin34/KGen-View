use super::*;
mod wndproc;
use wndproc::window_proc;

pub fn run() -> Result<(), WinError> {
    let _com_apartment = ComApartment::initialize()?;
    let launch_window = unsafe { GetForegroundWindow() };
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    // Win32 encodes integer resource identifiers in the low word of a pointer.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = unsafe {
        LoadIconW(Some(instance), PCWSTR(1usize as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))?
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: icon,
        hIconSm: icon,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(WinError::empty());
    }
    let settings = Settings::load();
    let capture_enabled = settings.capture_enabled;
    let paths = command_line_paths();
    let initial_archive_selection = paths
        .first()
        .and_then(|path| resolve_explorer_archive_selection(path, launch_window));
    let always_on_top = settings.always_on_top;
    let raw_state = Box::into_raw(Box::new(AppState::new(settings)?));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("KGen View"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            620,
            None,
            None,
            Some(instance),
            Some(raw_state.cast()),
        )
    };
    let hwnd = match hwnd {
        Ok(value) => value,
        Err(error) => {
            unsafe {
                drop(Box::from_raw(raw_state));
            }
            return Err(error);
        }
    };
    unsafe {
        if always_on_top {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }
    unsafe {
        state_mut(hwnd).begin_update_check();
    }
    if capture_enabled {
        unsafe {
            state_mut(hwnd).settings.save();
        }
        crate::platform::screenshot::stop_background();
        if let Err(error) = crate::platform::screenshot::start_background() {
            unsafe {
                state_mut(hwnd).settings.capture_enabled = false;
                state_mut(hwnd).settings.save();
                state_mut(hwnd).rebuild_menu();
            }
            fatal_error(&error);
        }
    }
    if std::env::args().any(|arg| arg == "--mcp") {
        unsafe {
            state_mut(hwnd).mcp_requests = Some(crate::mcp::start(hwnd.0 as isize));
        }
    }
    if let Some(selection) = initial_archive_selection {
        unsafe {
            state_mut(hwnd)
                .start_load_archive_member(selection.archive_path, selection.member_name);
        }
    } else if let Some(path) = paths.first() {
        unsafe {
            state_mut(hwnd).start_load(path.clone());
        }
    }
    if paths.len() > 1
        && let Ok(executable) = std::env::current_exe()
    {
        for path in paths.iter().skip(1) {
            let _ = std::process::Command::new(&executable).arg(path).spawn();
        }
    }
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { drop(Box::from_raw(raw_state)) };
    Ok(())
}

struct ComApartment;

impl ComApartment {
    pub(super) fn initialize() -> Result<Self, WinError> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn fatal_error(message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("KGen View"),
            MB_OK | MB_ICONERROR,
        );
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.file_service.shutdown();
        self.loader.shutdown();
        self.catalog_loader.shutdown();
        self.folder_watcher.shutdown();
    }
}
