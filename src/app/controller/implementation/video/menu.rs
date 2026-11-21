use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_DOWN, VK_RETURN, VK_TAB, VK_UP};

impl AppState {
    fn video_menu_ref(&self) -> Option<&VideoMenu> {
        self.video()?
            .panel
            .as_ref()?
            .menu
            .as_ref()
            .filter(|menu| menu.opened())
    }

    fn video_menu_mut(&mut self) -> Option<&mut VideoMenu> {
        self.video
            .as_deref_mut()
            .filter(|view| view.active)?
            .panel
            .as_mut()?
            .menu
            .as_mut()
    }

    pub(super) fn video_menu_opened(&self) -> bool {
        self.video_menu_ref().is_some()
    }

    pub(super) fn close_video_menu(&mut self) {
        if let Some(menu) = self.video_menu_mut() {
            menu.close();
        }
    }

    pub(super) fn video_menu(&mut self, action: Action) {
        let page = match action {
            Action::Rate => MenuPage::Speed,
            Action::Tracks => MenuPage::Tracks,
            _ => MenuPage::Playback,
        };
        let Some(panel) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active)
            .and_then(|view| view.panel.as_mut())
            .filter(|panel| panel.layout.is_some())
        else {
            return;
        };
        if panel.menu.is_none() {
            panel.menu = VideoMenu::new(self.hwnd, &self.d2d_factory).ok();
        }
        let Some(menu) = &mut panel.menu else { return };
        if menu.page == Some(page) {
            menu.close();
        } else {
            menu.open(page);
            panel.focus = None;
            if let Some(volume) = &mut panel.volume {
                volume.clear_input();
            }
        }
        self.refresh_video_menu();
        self.reveal_video_panel();
    }

    pub(super) fn refresh_video_menu(&mut self) {
        let Some(page) = self.video_menu_ref().and_then(|menu| menu.page) else {
            return;
        };
        let Some(view) = self.video() else { return };
        let status = view.display_status(Instant::now());
        let language = self.lang();
        let tr = |es, en| {
            if language == Language::Spanish {
                es
            } else {
                en
            }
        };
        let prefs = view.preferences;
        let media = view.media.as_deref();
        let tracks = media.map_or(&[][..], |media| media.audio.as_slice());
        let subtitles = media.map_or(&[][..], |media| media.subtitles.as_slice());
        let chapters = media.map_or(&[][..], |media| media.chapters.as_slice());
        let page_row = |name, detail, page| MenuRow::new(name, detail, MenuCommand::Page(page));
        let subtitles_available = subtitles.iter().any(|item| item.id >= 0);
        let track_rows = || {
            let mut rows = vec![page_row(tr("Audio", "Audio"), "", MenuPage::Audio)];
            if subtitles_available {
                rows.push(page_row(
                    tr("Subtítulos", "Subtitles"),
                    if status.subtitle_track >= 0 {
                        tr("Activos", "On")
                    } else {
                        tr("Desactivados", "Off")
                    },
                    MenuPage::Subtitles,
                ));
            }
            if status.seekable && !chapters.is_empty() {
                rows.push(MenuRow::new(
                    tr("Capítulos", "Chapters"),
                    chapters.len().to_string(),
                    MenuCommand::Page(MenuPage::Chapters),
                ));
            }
            rows
        };
        let (title, rows) = match page {
            MenuPage::Playback => {
                let mut rows = vec![
                    MenuRow::new(
                        tr("Velocidad", "Playback speed"),
                        rate_label(prefs.rate, language),
                        MenuCommand::Page(MenuPage::Speed),
                    ),
                    MenuRow::new(tr("Repetir vídeo", "Loop video"), "", MenuCommand::Loop)
                        .selected(prefs.looping),
                ];
                if status.video_ready && matches!(status.phase, Phase::Playing | Phase::Paused) {
                    rows.push(MenuRow::new(
                        tr("Siguiente fotograma", "Next frame"),
                        "E",
                        MenuCommand::NextFrame,
                    ));
                }
                if status.seekable
                    && let Some(position) = view.previous_position
                {
                    rows.push(MenuRow::new(
                        tr("Volver al punto anterior", "Return to previous position"),
                        time_label(position),
                        MenuCommand::PreviousPosition,
                    ));
                }
                rows.extend(track_rows());
                rows.push(page_row(tr("Archivo", "File"), "", MenuPage::File));
                (tr("Reproducción", "Playback"), rows)
            }
            MenuPage::Speed => (
                tr("Velocidad", "Playback speed"),
                RATES
                    .into_iter()
                    .map(|rate| {
                        MenuRow::new(rate_label(rate, language), "", MenuCommand::Rate(rate))
                            .selected(rate == prefs.rate)
                    })
                    .collect(),
            ),
            MenuPage::Tracks => (
                tr("Pistas y capítulos", "Tracks and chapters"),
                track_rows(),
            ),
            MenuPage::Audio => {
                let mut rows = vec![
                    MenuRow::new(
                        tr("Silenciar", "Mute"),
                        format!("{} %", prefs.volume),
                        MenuCommand::Mute,
                    )
                    .selected(prefs.muted),
                    MenuRow::new(
                        tr("Bajar volumen", "Volume down"),
                        "−5 %",
                        MenuCommand::Volume(-5),
                    ),
                    MenuRow::new(
                        tr("Subir volumen", "Volume up"),
                        "+5 %",
                        MenuCommand::Volume(5),
                    ),
                ];
                for (index, track) in tracks.iter().enumerate() {
                    let label = if track.id < 0 {
                        tr("Sin audio", "Disable audio").to_owned()
                    } else if track.name.is_empty() {
                        format!("{} {}", tr("Pista", "Track"), index + 1)
                    } else {
                        track.name.clone()
                    };
                    rows.push(
                        MenuRow::new(label, "", MenuCommand::Audio(track.id))
                            .selected(status.audio_track == track.id),
                    );
                }
                (tr("Audio", "Audio"), rows)
            }
            MenuPage::Subtitles => (
                tr("Subtítulos", "Subtitles"),
                subtitles
                    .iter()
                    .enumerate()
                    .map(|(index, track)| {
                        let label = if track.id < 0 {
                            tr("Desactivados", "Off").to_owned()
                        } else if track.name.is_empty() {
                            format!("{} {}", tr("Pista", "Track"), index + 1)
                        } else {
                            track.name.clone()
                        };
                        MenuRow::new(label, "", MenuCommand::Subtitle(track.id))
                            .selected(status.subtitle_track == track.id)
                    })
                    .collect(),
            ),
            MenuPage::Chapters => {
                let active = chapters
                    .partition_point(|chapter| chapter.start_ms <= status.elapsed_ms)
                    .checked_sub(1);
                let rows = chapters
                    .iter()
                    .enumerate()
                    .map(|(index, chapter)| {
                        let label = if chapter.name.is_empty() {
                            format!("{} {}", tr("Capítulo", "Chapter"), index + 1)
                        } else {
                            chapter.name.clone()
                        };
                        MenuRow::new(
                            label,
                            time_label(chapter.start_ms),
                            MenuCommand::Chapter(chapter.start_ms),
                        )
                        .selected(active == Some(index))
                    })
                    .collect();
                (tr("Capítulos", "Chapters"), rows)
            }
            MenuPage::File => (
                tr("Archivo", "File"),
                vec![
                    MenuRow::new(
                        tr("Abrir archivo…", "Open file…"),
                        "",
                        MenuCommand::File(cmd::OPEN),
                    ),
                    MenuRow::new(
                        tr("Mostrar en el Explorador", "Show in Explorer"),
                        "",
                        MenuCommand::File(cmd::OPEN_FOLDER),
                    ),
                    MenuRow::new(
                        tr("Copiar ruta", "Copy path"),
                        "",
                        MenuCommand::File(cmd::COPY_PATH),
                    ),
                    MenuRow::new(
                        tr("Menú de KGen View", "KGen View menu"),
                        "",
                        MenuCommand::ApplicationMenu,
                    ),
                ],
            ),
        };
        if let Some(menu) = self.video_menu_mut() {
            menu.set_rows(title, rows);
        }
    }

    pub(in crate::app::controller::implementation) fn video_menu_input(
        &mut self,
        message: u32,
        point: POINT,
    ) {
        let Some(menu) = self.video_menu_mut().filter(|menu| menu.opened()) else {
            return;
        };
        let hit = match message {
            WM_LBUTTONDOWN => {
                menu.press(point);
                None
            }
            WM_LBUTTONUP => menu.release(point),
            WM_CAPTURECHANGED => {
                menu.cancel_press();
                None
            }
            _ => None,
        };
        if let Some(hit) = hit {
            self.activate_menu_hit(hit);
        }
    }

    pub(super) fn video_menu_wheel_at(&mut self, mut screen: POINT, delta: i16) -> bool {
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut screen);
        }
        let Some(menu) = self
            .video_menu_mut()
            .filter(|menu| menu.local(screen).is_some())
        else {
            return false;
        };
        menu.scroll(if delta > 0 { -2 } else { 2 });
        self.render_video_panel();
        true
    }

    pub(in crate::app::controller::implementation) fn video_menu_key(
        &mut self,
        key: u32,
        shift: bool,
    ) -> bool {
        let Some(menu) = self.video_menu_mut().filter(|menu| menu.opened()) else {
            return false;
        };
        if key == VK_ESCAPE.0 as u32 {
            menu.close();
        } else if key == VK_BACK.0 as u32 {
            menu.back();
            self.refresh_video_menu();
        } else if key == VK_TAB.0 as u32 || key == VK_UP.0 as u32 || key == VK_DOWN.0 as u32 {
            menu.move_focus(
                if key == VK_UP.0 as u32 || (key == VK_TAB.0 as u32 && shift) {
                    -1
                } else {
                    1
                },
            );
        } else if key == VK_RETURN.0 as u32 {
            if let Some(hit) = menu.focused() {
                self.activate_menu_hit(hit);
            }
        } else {
            return false;
        }
        self.render_video_panel();
        true
    }

    fn activate_menu_hit(&mut self, hit: MenuHit) {
        let command = match hit {
            MenuHit::Close => {
                self.close_video_menu();
                self.render_video_panel();
                return;
            }
            MenuHit::Back => {
                if let Some(menu) = self.video_menu_mut() {
                    menu.back();
                }
                self.refresh_video_menu();
                self.render_video_panel();
                return;
            }
            MenuHit::Row(row) => self.video_menu_ref().and_then(|menu| menu.command_at(row)),
        };
        let Some(command) = command else { return };
        match command {
            MenuCommand::Page(page) => {
                if let Some(menu) = self.video_menu_mut() {
                    menu.change_page(page);
                }
            }
            MenuCommand::Rate(rate) => self.video_rate(rate),
            MenuCommand::Loop => self.video_loop(),
            MenuCommand::NextFrame => self.video_next_frame(),
            MenuCommand::PreviousPosition => {
                if let Some(position) = self.video().and_then(|view| view.previous_position) {
                    self.close_video_menu();
                    self.video_seek_to(position);
                }
            }
            MenuCommand::Volume(delta) => self.video_audio(delta, false),
            MenuCommand::Mute => self.video_audio(0, true),
            MenuCommand::Audio(id) => self.send_video_control(Control::AudioTrack(id)),
            MenuCommand::Subtitle(id) => self.send_video_control(Control::SubtitleTrack(id)),
            MenuCommand::Chapter(position) => {
                self.close_video_menu();
                self.video_seek_to(position);
            }
            MenuCommand::File(command) => {
                self.close_video_menu();
                self.handle_command(command);
            }
            MenuCommand::ApplicationMenu => {
                self.close_video_menu();
                let mut anchor = POINT::default();
                if let Some(menu) = &self.menu {
                    unsafe {
                        let _ = GetCursorPos(&mut anchor);
                        menu.show_all(self.hwnd, anchor.x, anchor.y);
                    }
                }
            }
        }
        self.refresh_video_menu();
        self.render_video_panel();
    }
}
