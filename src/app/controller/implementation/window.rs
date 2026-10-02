pub fn run() -> Result<(), WinError> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    if let Some(path) = command_line_path()
        && send_to_existing(&path)
    {
        unsafe { CoUninitialize() };
        return Ok(());
    }
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
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }
    if let Some(path) = command_line_path() {
        unsafe {
            state_mut(hwnd).start_load(path);
        }
    }
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { CoUninitialize() };
    Ok(())
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

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        let state = unsafe { (*create).lpCreateParams as *mut AppState };
        if !state.is_null() {
            unsafe {
                (*state).hwnd = hwnd;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            }
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState };
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_CREATE => {
            unsafe {
                state.create_controls();
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(target) = &state.target {
                let width = loword(lparam.0 as usize) as u32;
                let height = hiword(lparam.0 as usize) as u32;
                if width > 0
                    && height > 0
                    && unsafe { target.Resize(&D2D_SIZE_U { width, height }) }.is_err()
                {
                    state.target = None;
                    state.bitmap = None;
                    state.chrome.reset_render_resources();
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                state.rebuild_menu();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if state.settings.theme == ThemeMode::System {
                unsafe {
                    state.apply_theme();
                    state.rebuild_menu();
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
                state.paint(ps.hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MEASUREITEM => {
            let handled = state.menu.as_ref().is_some_and(|menu| unsafe {
                menu.measure(lparam.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT)
            });
            if handled {
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_DRAWITEM => {
            let handled = state.menu.as_ref().is_some_and(|menu| unsafe {
                menu.draw(lparam.0 as *mut windows::Win32::UI::Controls::DRAWITEMSTRUCT)
            });
            if handled {
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            state.handle_command(loword(wparam.0) as usize);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            handle_key(state, wparam.0 as u32);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(wparam.0) as i16;
            if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
                state.change_zoom(if delta > 0 { 1.15 } else { 1.0 / 1.15 });
            } else {
                state.navigate(if delta > 0 { -1 } else { 1 });
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            if state.chrome.menu.press(&layout, point).is_some()
                || state.chrome.toolbar.press(&layout, point).is_some()
            {
                unsafe {
                    SetCapture(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            } else if point.y >= state.image_area().top && point.y < state.image_area().bottom {
                state.selection_start = Some(point);
                state.selection_end = Some(point);
                unsafe {
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            let menu_changed = state.chrome.menu.update_hover(&layout, point);
            let toolbar_changed = state.chrome.toolbar.update_hover(&layout, point);
            if menu_changed || toolbar_changed {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            if state.selection_start.is_some() {
                state.selection_end = Some(point);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            if wparam.0 & 0x0010 != 0
                && let Some(previous) = state.middle_start.replace(point)
                && state.document.is_some()
            {
                state.zoom_mode = ZoomMode::Custom;
                state.center.0 -= (point.x - previous.x) as f32 / state.zoom;
                state.center.1 -= (point.y - previous.y) as f32 / state.zoom;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            let menu_interaction = state.chrome.menu.pressed.is_some();
            let toolbar_interaction = state.chrome.toolbar.pressed.is_some();
            let menu_index = state.chrome.menu.release(&layout, point);
            let command = state.chrome.toolbar.release(&layout, point);
            unsafe {
                let _ = ReleaseCapture();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            if menu_interaction {
                if let Some(index) = menu_index {
                    state.show_menu(index, &layout);
                }
            } else if toolbar_interaction {
                if let Some(command) = command {
                    state.handle_command(command);
                }
            } else {
                state.finish_selection();
            }
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            state.middle_start = Some(point_from_lparam(lparam));
            unsafe {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            state.middle_start = None;
            unsafe {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_DROPFILES => {
            let drop = windows::Win32::UI::Shell::HDROP(wparam.0 as *mut c_void);
            let length = unsafe { DragQueryFileW(drop, 0, None) };
            if length > 0 {
                let mut buffer = vec![0u16; length as usize + 1];
                unsafe {
                    DragQueryFileW(drop, 0, Some(&mut buffer));
                    DragFinish(drop);
                }
                let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
                if state.can_leave() {
                    state.start_load(path);
                }
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_COPYDATA => {
            let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            if data.dwData != IMAGE_HANDOFF_DATA || data.cbData < 2 || data.lpData.is_null() {
                return LRESULT(0);
            }
            let slice = unsafe {
                std::slice::from_raw_parts(data.lpData.cast::<u16>(), data.cbData as usize / 2)
            };
            let end = slice
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(slice.len());
            let incoming = PathBuf::from(String::from_utf16_lossy(&slice[..end]));
            if state
                .active_path
                .as_deref()
                .is_some_and(|active| paths_equal(active, &incoming))
            {
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_LOAD_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut LoadPayload) };
            unsafe {
                state.accept_load(*payload);
            }
            LRESULT(0)
        }
        WM_PREFETCH_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut PrefetchPayload) };
            state.accept_prefetch(*payload);
            LRESULT(0)
        }
        WM_CATALOG_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut CatalogPayload) };
            state.accept_catalog(*payload);
            LRESULT(0)
        }
        WM_UPLOAD_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut UploadPayload) };
            state.accept_upload(*payload);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATION {
                if let Some(doc) = &mut state.document {
                    doc.next_frame();
                    state.bitmap = None;
                    state.invalidate_image();
                    state.set_animation_timer();
                }
            } else if wparam.0 == TIMER_SLIDESHOW {
                state.navigate(1);
            } else if wparam.0 == TIMER_FOLDER_REFRESH
                && let Some(document) = &state.document
            {
                if folder_modified(&document.path) != state.catalog_stamp {
                    state.catalog_loader.submit(
                        state.generation,
                        document.path.clone(),
                        state.hwnd,
                    );
                }
            } else if wparam.0 == TIMER_AUTO_RESIZE {
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_AUTO_RESIZE);
                    if state.settings.auto_resize && !state.keeps_window_size() {
                        state.auto_resize_window();
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if state.can_leave() {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            state.alive.store(false, Ordering::Release);
            state.settings.save();
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn handle_key(state: &mut AppState, key: u32) {
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match key {
        k if k == VK_LEFT.0 as u32 => state.navigate(-1),
        k if k == VK_RIGHT.0 as u32 => state.navigate(1),
        k if k == VK_HOME.0 as u32 => state.navigate_edge(true),
        k if k == VK_END.0 as u32 => state.navigate_edge(false),
        k if k == VK_DELETE.0 as u32 => state.delete_current(),
        k if k == VK_F11.0 as u32 => state.toggle_fullscreen(),
        k if k == VK_F5.0 as u32 => state.toggle_slideshow(),
        k if k == VK_F9.0 as u32 => state.handle_command(cmd::RELOAD),
        k if k == VK_SPACE.0 as u32 => state.handle_command(cmd::PAUSE_ANIMATION),
        k if k == VK_PRIOR.0 as u32 => state.handle_command(cmd::PREVIOUS_FRAME),
        k if k == VK_NEXT.0 as u32 => state.handle_command(cmd::NEXT_FRAME),
        k if k == VK_ESCAPE.0 as u32 => {
            if state.fullscreen {
                state.toggle_fullscreen();
            }
        }
        k if k == VK_ADD.0 as u32 || k == VK_OEM_PLUS.0 as u32 => state.change_zoom(1.15),
        k if k == VK_SUBTRACT.0 as u32 || k == VK_OEM_MINUS.0 as u32 => {
            state.change_zoom(1.0 / 1.15)
        }
        0x31 if !ctrl => state.set_zoom(ZoomMode::Actual),
        0x46 if !ctrl => state.set_zoom(ZoomMode::Fit),
        0x49 if !ctrl => state.info(),
        0x4F if ctrl => state.open_dialog(),
        0x53 if ctrl && shift => {
            state.save_as_dialog();
        }
        0x43 if ctrl => state.copy_image(),
        0x58 if ctrl && shift => state.crop(),
        0x5A if ctrl => state.handle_command(cmd::UNDO),
        0x59 if ctrl => state.handle_command(cmd::REDO),
        _ => {}
    }
}

