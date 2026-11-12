use super::*;

impl AppState {
    pub(super) fn begin_update_check(&mut self) {
        if let Err(error) =
            updater::start_check(self.hwnd, self.alive.clone(), self.update_events.clone())
        {
            eprintln!("KGen View update check: {error}");
        }
    }

    pub(super) fn accept_update_event(&mut self) {
        let Some(event) = self.update_events.take() else {
            return;
        };
        match event {
            UpdateEvent::CheckComplete(Ok(update)) => {
                self.update_available = update;
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
            }
            UpdateEvent::CheckComplete(Err(error)) => {
                eprintln!("KGen View update check: {error}");
            }
            UpdateEvent::InstallStarted(Ok(())) => {
                self.update_download_busy = false;
                self.update_status(Some(tr(self.lang(), Text::InstallingUpdate)));
            }
            UpdateEvent::InstallStarted(Err(error)) => {
                self.update_download_busy = false;
                self.update_status(None);
                self.show_error(&error);
            }
        }
    }

    pub(super) fn begin_update_install(&mut self) {
        if self.update_download_busy {
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
        if let Err(error) = updater::start_install(
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
