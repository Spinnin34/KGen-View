use super::*;

impl AppState {
    pub(super) unsafe fn rebuild_menu(&mut self) {
        if let Ok(menu) = ThemedMenu::new(
            self.hwnd,
            self.lang(),
            self.settings.is_dark(),
            self.settings.always_on_top,
        ) {
            self.menu = Some(menu);
            let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
        }
    }

    pub(super) fn show_menu(&self, index: usize, layout: &ChromeLayout) {
        let Some(item) = layout.menu_items.iter().find(|item| item.index == index) else {
            return;
        };
        let mut anchor = POINT {
            x: item.rect.left.round() as i32,
            y: item.rect.bottom.round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut anchor);
            if let Some(menu) = &self.menu {
                if index == 6 {
                    menu.show_all(self.hwnd, anchor.x, anchor.y);
                } else {
                    menu.show_category(self.hwnd, index, anchor.x, anchor.y);
                }
            }
        }
    }

    pub(super) fn handle_command(&mut self, id: usize) {
        match id {
            cmd::OPEN => self.open_dialog(),
            cmd::SAVE_AS => {
                self.save_as_dialog();
            }
            cmd::CONVERT_IMAGE => self.convert_dialog(),
            cmd::ALWAYS_ON_TOP => {
                self.settings.always_on_top = !self.settings.always_on_top;
                self.settings.save();
                let insert_after = if self.settings.always_on_top {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(insert_after),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    self.rebuild_menu();
                }
            }
            cmd::EXIT => unsafe {
                let _ = SendMessageW(self.hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
            },
            cmd::UNDO => self.edit(|doc| {
                doc.undo();
            }),
            cmd::REDO => self.edit(|doc| {
                doc.redo();
            }),
            cmd::COPY => self.copy_image(),
            cmd::COPY_URL => self.copy_url(),
            cmd::CROP => self.crop(),
            cmd::DELETE => self.delete_current(),
            cmd::ROTATE_LEFT => self.edit(ImageDocument::rotate_left),
            cmd::ROTATE_RIGHT => self.edit(ImageDocument::rotate_right),
            cmd::FLIP_H => self.edit(ImageDocument::flip_horizontal),
            cmd::FLIP_V => self.edit(ImageDocument::flip_vertical),
            cmd::PREVIOUS => self.navigate(-1),
            cmd::NEXT => self.navigate(1),
            cmd::ACTUAL => self.set_zoom(ZoomMode::Actual),
            cmd::FIT => self.set_zoom(ZoomMode::Fit),
            cmd::FIT_WIDTH => self.set_zoom(ZoomMode::FitWidth),
            cmd::FULLSCREEN => self.toggle_fullscreen(),
            cmd::SLIDESHOW => self.toggle_slideshow(),
            cmd::PAUSE_ANIMATION => {
                self.animation_playing = !self.animation_playing;
                self.set_animation_timer();
                self.update_status(None);
            }
            cmd::RELOAD => {
                if let Some(doc) = &self.document {
                    let path = doc.path.clone();
                    self.cache.clear();
                    self.cache_bytes = 0;
                    self.request_leave(PendingAction::Load(path));
                }
            }
            cmd::INFO => self.info(),
            cmd::OPEN_FOLDER => self.open_folder(),
            cmd::COPY_PATH => self.copy_path(),
            cmd::PREVIOUS_FRAME => self.change_frame(-1),
            cmd::NEXT_FRAME => self.change_frame(1),
            cmd::LANGUAGE => {
                self.settings.language = self.settings.language.toggle();
                self.settings.save();
                unsafe {
                    self.rebuild_menu();
                }
                self.update_title();
                self.update_status(None);
            }
            cmd::THEME => {
                self.settings.cycle_theme();
                unsafe {
                    self.apply_theme();
                    self.rebuild_menu();
                }
            }
            cmd::ABOUT => {
                let text = wide(
                    "KGen View 1.2.0\n\nVisor nativo para Windows\nRust + WIC + Direct2D\nLicencia MIT\n\nSin telemetría. La red solo se usa al pulsar Copiar enlace (ImgBB).",
                );
                let caption = wide("KGen View");
                unsafe {
                    MessageBoxW(
                        Some(self.hwnd),
                        PCWSTR(text.as_ptr()),
                        PCWSTR(caption.as_ptr()),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
            }
            _ => {}
        }
    }
}
