use super::*;

impl AppState {
    pub(super) fn begin_update_check(&mut self) {
        if self.update_check_busy
            || self.update_available.is_some()
            || self.update_download_busy
            || self.install_on_exit.is_some()
            || !updater::is_installed_copy()
        {
            return;
        }

        match updater::start_check(self.hwnd, self.alive.clone(), self.update_events.clone()) {
            Ok(()) => self.update_check_busy = true,
            Err(error) => eprintln!("KGen View update check: {error}"),
        }
    }

    pub(super) fn accept_update_event(&mut self) {
        let Some(event) = self.update_events.take() else {
            return;
        };
        match event {
            UpdateEvent::CheckComplete(result) => {
                self.update_check_busy = false;
                match result {
                    Ok(update) => {
                        self.update_available = update;
                        self.layout_video_panel();
                        if self.update_available.is_some() {
                            self.reveal_video_panel();
                        }
                        unsafe {
                            let _ = InvalidateRect(Some(self.hwnd), None, false);
                        }
                    }
                    Err(error) => eprintln!("KGen View update check: {error}"),
                }
            }
            UpdateEvent::DownloadComplete(result) => {
                self.update_download_busy = false;
                self.update_status(None);
                match result {
                    Ok(installer) => self.request_leave(PendingAction::Install(installer)),
                    Err(error) => self.show_error(&error),
                }
            }
        }
    }

    pub(super) fn install_after_close(&mut self, installer: updater::VerifiedInstaller) {
        let path = self.current_path();
        let member = path
            .and_then(|path| self.active_archive_member(path))
            .map(|(archive, member)| (archive.source_path(), member));
        let install = match installer.reopen(path, member) {
            Ok(install) => install,
            Err(error) => {
                self.show_error(&error);
                return;
            }
        };
        self.install_on_exit = Some(install);
        self.update_status(Some(tr(self.lang(), Text::InstallingUpdate)));
        self.perform_action(PendingAction::Close);
    }

    pub(super) fn begin_update_install(&mut self) {
        if self.update_download_busy
            || self.install_on_exit.is_some()
            || matches!(self.pending_action, Some(PendingAction::Install(_)))
        {
            return;
        }
        let Some(update) = self.update_available.clone() else {
            return;
        };
        self.update_download_busy = true;
        let message = format!(
            "{} {} ({})",
            tr(self.lang(), Text::DownloadingUpdate),
            update.version,
            update.channel
        );
        self.update_status(Some(&message));
        if let Err(error) = updater::start_download(
            self.hwnd,
            self.alive.clone(),
            self.update_events.clone(),
            update,
        ) {
            self.update_download_busy = false;
            self.update_status(None);
            self.show_error(&error);
        }
    }
}
