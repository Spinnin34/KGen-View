use super::*;

impl AppState {
    pub(super) fn active_archive_member<'a>(
        &'a self,
        path: &Path,
    ) -> Option<(&'a ArchiveSession, &'a str)> {
        let archive = self.archive_sessions.get(self.active_archive?)?;
        Some((archive, archive.member_name(path)?))
    }

    pub(super) fn display_file_location(&self, path: &Path) -> String {
        self.active_archive_member(path)
            .map(|(archive, member)| format!("{}::{member}", display_path(archive.source_path())))
            .unwrap_or_else(|| display_path(path))
    }

    pub(super) fn image_position(&self) -> Option<(usize, usize)> {
        if let Some(index) = self.active_archive {
            let archive = self.archive_sessions.get(index)?;
            let cursor = self.archive_cursor?;
            return (cursor < archive.member_count())
                .then_some((cursor + 1, archive.member_count()));
        }
        self.catalog
            .index
            .map(|index| (index + 1, self.catalog.files.len()))
    }

    pub(super) fn adjacent_image_path(&self, offset: isize) -> Option<PathBuf> {
        if let Some(index) = self.active_archive {
            let archive = self.archive_sessions.get(index)?;
            return archive.member_path_adjacent(self.archive_cursor?, offset);
        }
        self.catalog.adjacent_wrapped(offset)
    }

    pub(super) fn edge_image_path(&self, first: bool) -> Option<PathBuf> {
        if let Some(index) = self.active_archive {
            let archive = self.archive_sessions.get(index)?;
            return if first {
                archive.first_member_path()
            } else {
                archive.last_member_path()
            };
        }
        if first {
            self.catalog.first()
        } else {
            self.catalog.last()
        }
    }

    pub(super) fn next_unfailed_image_path(&self, direction: isize) -> Option<PathBuf> {
        if let Some(index) = self.active_archive {
            let archive = self.archive_sessions.get(index)?;
            return archive.next_member_path_skipping(self.archive_cursor?, direction, |path| {
                self.failed_paths.contains(path)
            });
        }
        (1..=self.catalog.files.len()).find_map(|offset| {
            self.catalog
                .adjacent_wrapped(direction.signum().saturating_mul(offset as isize))
                .filter(|candidate| !self.failed_paths.contains(candidate))
        })
    }

    pub(super) fn accept_catalog(&mut self, payload: CatalogPayload) {
        if payload.generation != self.generation {
            return;
        }
        let is_pending_request = self
            .pending_catalog
            .as_ref()
            .is_some_and(|(generation, path)| {
                *generation == payload.generation && path == &payload.path
            });
        let is_active_document = self
            .document
            .as_ref()
            .is_some_and(|document| document.path == payload.path);
        if !is_pending_request && !is_active_document {
            return;
        }
        if is_pending_request {
            self.pending_catalog = None;
        }
        self.catalog = payload.catalog;
        self.catalog_stamp = folder_modified(&payload.path);
        if self.load_started.is_some() || !is_active_document {
            return;
        }
        self.update_status(None);
        if self.prefetch_enabled {
            self.start_prefetch();
        }
    }

    pub(super) fn schedule_auto_resize(&self) {
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

    pub(super) unsafe fn auto_resize_window(&mut self) {
        if self.keeps_window_size() {
            return;
        }
        let Some(document) = &self.document else {
            return;
        };
        let (image_width, image_height) = document.view_dimensions();
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
        let scale = (max_image_w / image_width.max(1) as f32)
            .min(max_image_h / image_height.max(1) as f32)
            .min(1.0);
        let desired_client_w = (image_width as f32 * scale).round() as i32;
        let desired_client_h = (image_height as f32 * scale).round() as i32 + chrome_h;
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

    pub(super) fn update_title(&self) {
        let title = self.document.as_ref().map_or_else(
            || "KGen View".into(),
            |document| {
                if let Some((archive, member)) = self.active_archive_member(&document.path) {
                    let archive_name = archive
                        .source_path()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    format!("{archive_name} — {member} — KGen View")
                } else {
                    document
                        .path
                        .file_name()
                        .map(|name| format!("{} — KGen View", name.to_string_lossy()))
                        .unwrap_or_else(|| "KGen View".into())
                }
            },
        );
        let wide = wide(&title);
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
        }
    }

    pub(super) fn navigate(&mut self, offset: isize) {
        if let Some(path) = self.adjacent_image_path(offset) {
            self.prefetch_enabled = true;
            self.navigation_direction = offset.signum();
            self.request_leave(PendingAction::Load(path));
        }
    }
    pub(super) fn navigate_edge(&mut self, first: bool) {
        let path = self.edge_image_path(first);
        if let Some(path) = path {
            self.prefetch_enabled = true;
            self.navigation_direction = if first { 1 } else { -1 };
            self.request_leave(PendingAction::Load(path));
        }
    }
    pub(super) fn perform_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::Load(path) => self.start_load(path),
            PendingAction::Close => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
        }
    }
    pub(super) fn request_leave(&mut self, action: PendingAction) {
        if self.file_busy {
            self.pending_action = Some(action);
            return;
        }
        if !self.document.as_ref().is_some_and(|doc| doc.dirty) {
            self.perform_action(action);
            return;
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
            6 => {
                if self.save_as_dialog() {
                    self.pending_action = Some(action);
                }
            }
            7 => self.perform_action(action),
            _ => {}
        }
    }
}
