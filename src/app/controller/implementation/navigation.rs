impl AppState {
    fn accept_catalog(&mut self, payload: CatalogPayload) {
        if payload.generation != self.generation
            || self.load_started.is_some()
            || self
                .document
                .as_ref()
                .is_none_or(|document| document.path != payload.path)
        {
            return;
        }
        self.catalog = payload.catalog;
        self.catalog_stamp = folder_modified(&payload.path);
        self.update_status(None);
        if self.prefetch_enabled {
            self.start_prefetch();
        }
    }

    fn schedule_auto_resize(&self) {
        if self.keeps_window_size() {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_AUTO_RESIZE);
            let _ = SetTimer(
                Some(self.hwnd),
                TIMER_AUTO_RESIZE,
                AUTO_RESIZE_DELAY_MS,
                None,
            );
        }
    }

    unsafe fn auto_resize_window(&mut self) {
        if self.keeps_window_size() {
            return;
        }
        let Some(document) = &self.document else {
            return;
        };
        let frame = document.current_frame();
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return;
        }
        let max_w = info.rcWork.right - info.rcWork.left;
        let max_h = info.rcWork.bottom - info.rcWork.top;
        let chrome = self.chrome_layout();
        let mut frame_probe = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let _ = unsafe {
            AdjustWindowRectEx(
                &mut frame_probe,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            )
        };
        let frame_w = frame_probe.right - frame_probe.left - 100;
        let frame_h = frame_probe.bottom - frame_probe.top - 100;
        let chrome_h = chrome.toolbar_height() + chrome.status_height();
        let max_image_w = (max_w - frame_w).max(1) as f32;
        let max_image_h = (max_h - frame_h - chrome_h).max(1) as f32;
        let scale = (max_image_w / frame.width.max(1) as f32)
            .min(max_image_h / frame.height.max(1) as f32)
            .min(1.0);
        let desired_client_w = (frame.width as f32 * scale).round() as i32;
        let desired_client_h = (frame.height as f32 * scale).round() as i32 + chrome_h;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: desired_client_w.max(220),
            bottom: desired_client_h.max(160),
        };
        let _ = unsafe {
            AdjustWindowRectEx(
                &mut outer,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            )
        };
        let width = (outer.right - outer.left).min(max_w).max(220);
        let height = (outer.bottom - outer.top).min(max_h).max(150);
        let mut current = RECT::default();
        let _ = unsafe { GetWindowRect(self.hwnd, &mut current) };
        let x = current.left.clamp(
            info.rcWork.left,
            (info.rcWork.right - width).max(info.rcWork.left),
        );
        let y = current.top.clamp(
            info.rcWork.top,
            (info.rcWork.bottom - height).max(info.rcWork.top),
        );
        if current.left == x
            && current.top == y
            && current.right - current.left == width
            && current.bottom - current.top == height
        {
            return;
        }
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }

    fn update_title(&self) {
        let title = self
            .document
            .as_ref()
            .and_then(|doc| doc.path.file_name())
            .map(|name| format!("{} — KGen View", name.to_string_lossy()))
            .unwrap_or_else(|| "KGen View".into());
        let wide = wide(&title);
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
        }
    }

    fn navigate(&mut self, offset: isize) {
        let Some(path) = self.catalog.adjacent_wrapped(offset) else {
            return;
        };
        if self.can_leave() {
            // Advance the logical position immediately. A fast wheel gesture
            // can then skip over images still being decoded and the worker
            // only has to finish the newest destination.
            self.prefetch_enabled = true;
            self.navigation_direction = offset.signum();
            self.catalog.update_index(&path);
            self.start_load(path);
        }
    }

    fn navigate_edge(&mut self, first: bool) {
        let path = if first {
            self.catalog.first()
        } else {
            self.catalog.last()
        };
        if let Some(path) = path
            && self.can_leave()
        {
            self.prefetch_enabled = true;
            self.navigation_direction = if first { 1 } else { -1 };
            self.catalog.update_index(&path);
            self.start_load(path);
        }
    }

    fn can_leave(&mut self) -> bool {
        if !self.document.as_ref().is_some_and(|doc| doc.dirty) {
            return true;
        }
        let caption = wide(tr(self.lang(), Text::AppTitle));
        let text = wide(tr(self.lang(), Text::SaveChanges));
        let answer = unsafe {
            MessageBoxW(
                Some(self.hwnd),
                PCWSTR(text.as_ptr()),
                PCWSTR(caption.as_ptr()),
                MB_YESNOCANCEL | MB_ICONQUESTION,
            )
        };
        match answer.0 {
            6 => self.save_as_dialog(),
            7 => true,
            _ => false,
        }
    }
}

