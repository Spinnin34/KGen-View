use super::*;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};

struct WindowFit {
    work: RECT,
    frame: (i32, i32),
    chrome_height: i32,
    dpi: u32,
}

impl WindowFit {
    fn content_limit(&self) -> (u32, u32) {
        (
            (self.work.right - self.work.left - self.frame.0).max(1) as u32,
            (self.work.bottom - self.work.top - self.frame.1 - self.chrome_height).max(1) as u32,
        )
    }
}

impl AppState {
    pub(super) fn current_path(&self) -> Option<&Path> {
        self.document
            .as_ref()
            .map(|doc| doc.path.as_path())
            .or(self.active_path.as_deref())
    }

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
        let is_active_document = self.current_path() == Some(payload.path.as_path());
        if !is_pending_request && !is_active_document {
            return;
        }
        if is_pending_request {
            self.pending_catalog = None;
        }
        self.catalog = payload.catalog;
        self.catalog_stamp = folder_modified(&payload.path);
        if is_pending_request && let Some(error) = self.deferred_load_error.take() {
            self.accept_load(LoadPayload {
                generation: payload.generation,
                path: payload.path,
                result: Err(error),
                archive: None,
            });
            return;
        }
        if self.load_started.is_some() || !is_active_document {
            return;
        }
        if self.video().is_some() {
            self.render_video_panel();
            return;
        }
        self.update_status(None);
        if self.prefetch_enabled {
            self.start_prefetch();
        }
    }

    fn window_fit(&self, video: bool) -> Option<WindowFit> {
        if !self.settings.auto_resize
            || self.keeps_window_size()
            || unsafe { IsIconic(self.hwnd).as_bool() }
        {
            return None;
        }
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        let mut frame = RECT::default();
        unsafe {
            AdjustWindowRectExForDpi(
                &mut frame,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
                dpi,
            )
            .ok()?;
        }
        Some(WindowFit {
            work: info.rcWork,
            frame: (frame.right - frame.left, frame.bottom - frame.top),
            chrome_height: if video {
                0
            } else {
                ChromeLayout::chrome_height(dpi)
            },
            dpi,
        })
    }

    pub(super) fn opening_preview_target(&self) -> (u32, u32) {
        self.window_fit(false).map_or_else(
            || {
                let area = self.image_area();
                (
                    (area.right - area.left).max(1) as u32,
                    (area.bottom - area.top).max(1) as u32,
                )
            },
            |fit| fit.content_limit(),
        )
    }

    pub(super) fn fit_open_media(&self, dimensions: (u32, u32), video: bool) {
        let Some(fit) = self.window_fit(video) else {
            return;
        };
        let (image_width, image_height) =
            crate::imaging::fitted_size(dimensions, fit.content_limit(), usize::MAX);
        let (min_width, min_height) = if video {
            ((336 * fit.dpi / 96) as i32, (190 * fit.dpi / 96) as i32)
        } else {
            (220, 160)
        };
        let width =
            ((image_width as i32).max(min_width) + fit.frame.0).min(fit.work.right - fit.work.left);
        let height = ((image_height as i32 + fit.chrome_height).max(min_height) + fit.frame.1)
            .min(fit.work.bottom - fit.work.top);
        let mut current = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut current) }.is_err() {
            return;
        }
        let left = current.left.clamp(fit.work.left, fit.work.right - width);
        let top = current.top.clamp(fit.work.top, fit.work.bottom - height);
        if current
            != (RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            })
        {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    left,
                    top,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
    }

    pub(super) fn update_title(&self) {
        let title = self.active_path.as_deref().map_or_else(
            || "KGen View".into(),
            |path| {
                if let Some((archive, member)) = self.active_archive_member(path) {
                    let archive_name = archive
                        .source_path()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    format!("{archive_name} — {member} — KGen View")
                } else {
                    path.file_name()
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
        self.cancel_wheel_navigation();
        if let Some(path) = self.adjacent_image_path(offset) {
            self.prefetch_enabled = true;
            self.navigation_direction = offset.signum();
            self.request_leave(PendingAction::Load(path));
        }
    }

    pub(super) fn navigate_wheel(&mut self, offset: isize) {
        // Coalesce wheel input without moving the cursor used by the arrows,
        // prefetch and the status bar before a load actually starts.
        let pending_target = self.pending_wheel_path.as_deref().and_then(|path| {
            if let Some(index) = self.active_archive {
                let archive = self.archive_sessions.get(index)?;
                archive.member_path_adjacent(archive.member_index(path)?, offset)
            } else {
                self.catalog.adjacent_from(path, offset)
            }
        });
        let Some(path) = pending_target.or_else(|| self.adjacent_image_path(offset)) else {
            return;
        };
        self.prefetch_enabled = true;
        self.navigation_direction = offset.signum();

        if self.active_path.as_ref() == Some(&path) {
            self.cancel_wheel_navigation();
            return;
        }

        let cached = self.cache.iter().any(|(cached, _)| cached == &path);
        let has_unsaved_changes = self.document.as_ref().is_some_and(|doc| doc.dirty);
        if cached || has_unsaved_changes {
            self.cancel_wheel_navigation();
            self.request_leave(PendingAction::Load(path));
            return;
        }

        self.pending_wheel_path = Some(path);
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_WHEEL_NAVIGATION);
            if SetTimer(
                Some(self.hwnd),
                TIMER_WHEEL_NAVIGATION,
                WHEEL_NAVIGATION_COALESCE_MS,
                None,
            ) == 0
            {
                self.flush_wheel_navigation();
            }
        }
    }

    pub(super) fn flush_wheel_navigation(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_WHEEL_NAVIGATION);
        }
        if let Some(path) = self.pending_wheel_path.take() {
            self.request_leave(PendingAction::Load(path));
        }
    }

    pub(super) fn cancel_wheel_navigation(&mut self) {
        self.pending_wheel_path = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_WHEEL_NAVIGATION);
        }
    }

    pub(super) fn navigate_edge(&mut self, first: bool) {
        self.cancel_wheel_navigation();
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
            PendingAction::Install(installer) => self.install_after_close(installer),
            PendingAction::Close => {
                if !self.close_video() {
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                }
            }
        }
    }
    pub(super) fn request_leave(&mut self, action: PendingAction) {
        self.cancel_wheel_navigation();
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
