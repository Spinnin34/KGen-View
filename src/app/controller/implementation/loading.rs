use super::*;

impl AppState {
    pub(super) fn start_load(&mut self, path: PathBuf) {
        self.start_load_with_archive_member(path, None);
    }

    pub(super) fn start_load_archive_member(&mut self, archive_path: PathBuf, member_name: String) {
        self.start_load_with_archive_member(archive_path, Some(member_name));
    }

    pub(super) fn start_load_with_archive_member(
        &mut self,
        path: PathBuf,
        initial_archive_member: Option<String>,
    ) {
        if self.video.as_ref().is_some_and(|view| view.closing) {
            return;
        }
        let started = Instant::now();
        self.cancel_wheel_navigation();
        // Keep a completed look-ahead result if its wakeup is still queued
        // when the user navigates again.
        if let Some(payload) = self.loader.take_prefetch() {
            self.accept_prefetch(payload);
            self.loader.ack_prefetch();
        }
        let opening_archive = archive::is_archive_path(&path);
        let known_archive = if opening_archive {
            None
        } else {
            self.archive_sessions
                .iter()
                .position(|archive| archive.contains_path(&path))
        };
        if !opening_archive && self.catalog.contains(&path) {
            self.catalog.update_index(&path);
        }
        // Catalog entries have already been normalized by the initial scan.
        // Archive member paths are session identities; canonicalizing them can
        // change their spelling and break that identity on Windows.
        let path = if known_archive.is_some() || self.catalog.contains(&path) {
            path
        } else {
            path.canonicalize().unwrap_or(path)
        };
        self.generation = self.generation.wrapping_add(1);
        self.load_started = Some(started);
        self.first_visible_cache_hit = false;
        let generation = self.generation;
        // Invalidate active/queued background work even when this target is
        // already cached and no replacement decode or catalog scan is needed.
        self.loader.invalidate(generation);
        self.catalog_loader.invalidate(generation);
        self.pending_catalog = None;
        self.view_pending = false;
        self.view_key = None;
        self.requested_page = 0;

        if opening_archive {
            if self.archive_fallback.is_none() {
                self.archive_fallback = Some(ArchiveFallback {
                    catalog: self.catalog.clone(),
                    active_archive: self.active_archive,
                    archive_cursor: self.archive_cursor,
                    catalog_stamp: self.catalog_stamp,
                });
            }
        } else {
            self.active_archive = known_archive;
            self.archive_cursor = self.active_archive.and_then(|index| {
                self.archive_sessions
                    .get(index)
                    .and_then(|archive| archive.member_index(&path))
            });
            if self.active_archive.is_none() {
                self.archive_fallback = None;
            }
        }
        let archive_member = if opening_archive {
            None
        } else {
            self.active_archive
                .and_then(|index| self.archive_sessions.get(index))
                .and_then(|archive| archive.member_ref(&path))
        };

        let is_archive_member = self.active_archive.is_some();
        let kind = MediaKind::of(&path);
        if !opening_archive && !is_archive_member && !self.catalog.matches(&path, kind) {
            if self.catalog.kind() != kind {
                self.catalog = FolderCatalog::default();
                self.catalog_stamp = None;
            }
            self.pending_catalog = Some((generation, path.clone()));
            self.catalog_loader
                .submit(generation, path.clone(), self.hwnd);
        }
        if !opening_archive && !is_archive_member && kind == MediaKind::Video {
            self.start_video(path);
            return;
        }
        self.stop_video();

        let cached_index = self.cache.iter().position(|(cached, _)| *cached == path);
        let cached_document = cached_index.map(|index| {
            let (_, document) = self.cache.remove(index);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&document));
            document
        });
        if let Some(document) = cached_document {
            self.load_time_ms = 0;
            self.first_visible_cache_hit = true;
            self.first_visible = self.load_started.take();
            let mut catalog = self.catalog.clone();
            if !catalog.matches(&path, MediaKind::Image) {
                catalog = FolderCatalog::default();
            } else {
                catalog.update_index(&path);
            }
            unsafe {
                self.accept_load(LoadPayload {
                    generation,
                    path,
                    result: Ok(document),
                    catalog,
                    archive: None,
                });
            }
            return;
        }

        self.update_status(Some(tr(self.lang(), Text::Loading)));
        self.loader.submit(
            generation,
            path,
            archive_member,
            initial_archive_member,
            self.hwnd,
            self.lang(),
            DecodeRequest::preview(self.opening_preview_target()),
        );
    }

    pub(super) unsafe fn accept_load(&mut self, payload: LoadPayload) {
        if payload.generation != self.generation {
            return;
        }
        let LoadPayload {
            path,
            result,
            catalog,
            archive,
            ..
        } = payload;
        let archive_open_error = archive::is_archive_path(&path) && archive.is_none();
        let loaded_archive_index = if let Some(archive) = archive {
            self.cache.clear();
            self.cache_bytes = 0;
            self.archive_sessions.push(archive);
            Some(self.archive_sessions.len() - 1)
        } else {
            None
        };
        self.active_archive = loaded_archive_index.or_else(|| {
            self.archive_sessions
                .iter()
                .position(|archive| archive.contains_path(&path))
        });
        self.archive_cursor = self.active_archive.and_then(|index| {
            self.archive_sessions
                .get(index)
                .and_then(|archive| archive.member_index(&path))
        });
        if result.is_ok() {
            self.archive_fallback = None;
        }
        match result {
            Ok(mut document) => {
                // The decoder canonicalizes file paths for I/O. Keep the
                // archive session's exact member path as the document identity
                // so navigation, restart registration and ZIP metadata keep
                // resolving to the active member on Windows.
                if self.active_archive.is_some() {
                    document.path = path.clone();
                }
                if self.load_started.is_some() {
                    self.first_visible = self.load_started;
                }
                if self.load_started.is_some() {
                    self.load_time_ms = self
                        .load_started
                        .take()
                        .map(|started| started.elapsed().as_millis())
                        .unwrap_or(0);
                }
                let is_archive_member = self.active_archive.is_some();
                let needs_catalog = if is_archive_member {
                    self.catalog = FolderCatalog::default();
                    self.catalog_stamp = None;
                    false
                } else {
                    let is_same_folder = self.catalog.matches(&path, MediaKind::Image);
                    let needs_catalog = catalog.files.is_empty() && !is_same_folder;
                    if catalog.files.is_empty() || is_same_folder {
                        if is_same_folder {
                            self.catalog.update_index(&path);
                        } else {
                            self.catalog = FolderCatalog::default();
                            self.catalog_stamp = None;
                        }
                    } else {
                        self.catalog = catalog;
                        self.catalog_stamp = folder_modified(&path);
                    }
                    needs_catalog
                };
                self.failed_paths.remove(&path);
                self.active_path = Some(path.clone());
                let (width, height) = document.view_dimensions();
                self.center = (width as f32 / 2.0, height as f32 / 2.0);
                self.requested_page = document.frame_index;
                self.document = Some(document);
                self.retain_active_archive_session();
                self.refresh_restart_registration();
                self.folder_watcher.watch(if self.active_archive.is_some() {
                    None
                } else {
                    path.parent().map(Path::to_path_buf)
                });
                if self.folder_dirty && !is_archive_member {
                    self.folder_dirty = false;
                    self.catalog_loader
                        .submit(self.generation, path.clone(), self.hwnd);
                } else if is_archive_member {
                    self.folder_dirty = false;
                }
                self.zoom_mode = ZoomMode::Fit;
                self.crop_selection = None;
                self.bitmaps.clear();
                self.set_animation_timer();
                self.update_title();
                self.fit_open_media((width, height), false);
                self.invalidate_image();
                self.present_startup_window();
                self.prepare_opened_image_view((width, height));
                if needs_catalog {
                    let catalog_already_requested = self.pending_catalog.as_ref().is_some_and(
                        |(generation, requested_path)| {
                            *generation == self.generation && requested_path == &path
                        },
                    );
                    if !catalog_already_requested {
                        self.pending_catalog = Some((self.generation, path.clone()));
                        self.catalog_loader
                            .submit(self.generation, path.clone(), self.hwnd);
                    }
                } else if self.prefetch_enabled {
                    self.start_prefetch();
                }
            }
            Err(error) => {
                if let Some(started) = self.load_started.take() {
                    self.load_time_ms = started.elapsed().as_millis();
                }
                if archive_open_error {
                    self.restore_archive_fallback();
                    self.update_title();
                    self.update_status(None);
                    self.present_startup_window();
                    self.show_error(&error.to_string());
                    return;
                }
                let archive_source_changed = self
                    .active_archive
                    .and_then(|index| self.archive_sessions.get(index))
                    .is_some_and(|archive| !archive.source_is_current());
                if archive_source_changed {
                    self.restore_archive_fallback();
                    self.update_title();
                    self.update_status(None);
                    self.present_startup_window();
                    self.show_error(&error.to_string());
                    return;
                }
                if self.active_archive.is_some() {
                    // A ZIP member is never a file in the temporary extraction
                    // directory for navigation or folder-catalog purposes.
                    self.catalog = FolderCatalog::default();
                    self.catalog_stamp = None;
                } else {
                    let is_same_folder = self.catalog.matches(&path, MediaKind::Image);
                    if catalog.files.is_empty() && is_same_folder {
                        self.catalog.update_index(&path);
                    } else {
                        self.catalog = catalog;
                        self.catalog_stamp = folder_modified(&path);
                    }
                }
                self.failed_paths.insert(path.clone());
                let extension = path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let suffix = if matches!(extension.as_str(), "webp" | "heic" | "heif") {
                    format!("\n\n{}", tr(self.lang(), Text::CodecMissing))
                } else {
                    String::new()
                };
                let next = self.next_unfailed_image_path(self.navigation_direction);
                if let Some(next) = next {
                    self.update_status(Some(if self.lang() == Language::Spanish {
                        "Imagen dañada o incompatible; pasando a la siguiente…"
                    } else {
                        "Damaged or unsupported image; skipping to the next…"
                    }));
                    self.start_load(next);
                } else {
                    self.restore_archive_fallback();
                    self.update_title();
                    self.update_status(Some(tr(self.lang(), Text::Unsupported)));
                    self.present_startup_window();
                    self.show_error(&format!("{}{}", error, suffix));
                }
            }
        }
    }

    fn prepare_opened_image_view(&self, size: (u32, u32)) {
        let area = self.image_area();
        let target = (
            (area.right - area.left).max(1) as u32,
            (area.bottom - area.top).max(1) as u32,
        );
        let needed = DecodeRequest::preview(target).layout(size).size;
        if self.document.as_ref().is_some_and(|document| {
            let frame = document.current_frame();
            document.rendered_region == (0, 0, size.0, size.1)
                && frame.width.saturating_add(1) >= needed.0
                && frame.height.saturating_add(1) >= needed.1
        }) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_VIEW);
            }
        } else {
            self.schedule_view();
        }
    }

    pub(super) fn start_prefetch(&self) {
        if unsafe { IsIconic(self.hwnd).as_bool() } {
            return;
        }
        // Keep the next two bounded previews ready in the current direction.
        // At 1920x1080 they fit inside the existing 16 MiB cache budget.
        let direction = if self.navigation_direction < 0 { -1 } else { 1 };
        let candidates: Vec<DecodeTarget> = (1..=2)
            .map(|step| direction * step)
            .filter_map(|offset| self.adjacent_image_path(offset))
            .filter(|path| self.active_path.as_ref() != Some(path))
            .filter(|path| !self.cache.iter().any(|(cached, _)| cached == path))
            .filter_map(|path| {
                let archive_member = self
                    .active_archive
                    .and_then(|index| self.archive_sessions.get(index))
                    .and_then(|archive| archive.member_ref(&path));
                if archive_member
                    .as_ref()
                    .is_some_and(|member| !member.can_prefetch())
                {
                    return None;
                }
                Some(DecodeTarget {
                    path,
                    archive_member,
                })
            })
            .collect();
        self.loader
            .submit_prefetch(self.generation, candidates, self.hwnd, self.lang());
    }

    pub(super) fn accept_prefetch(&mut self, payload: PrefetchPayload) {
        if payload.generation != self.generation
            || self.cache.iter().any(|(path, _)| *path == payload.path)
        {
            return;
        }
        let Ok(document) = payload.result else { return };
        self.cache_document(payload.path, document);
    }

    pub(super) fn retain_active_archive_session(&mut self) {
        if let Some(index) = self.active_archive {
            if index < self.archive_sessions.len() {
                let active = self.archive_sessions.remove(index);
                self.archive_sessions.clear();
                self.archive_sessions.push(active);
                self.active_archive = Some(0);
            } else {
                self.archive_sessions.clear();
                self.active_archive = None;
                self.archive_cursor = None;
            }
        } else {
            self.archive_sessions.clear();
            self.archive_cursor = None;
        }
    }

    fn restore_archive_fallback(&mut self) {
        if let Some(fallback) = self.archive_fallback.take() {
            self.catalog = fallback.catalog;
            self.active_archive = fallback.active_archive;
            self.archive_cursor = fallback.archive_cursor;
            self.catalog_stamp = fallback.catalog_stamp;
            self.retain_active_archive_session();
        }
    }

    pub(super) fn refresh_restart_registration(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        let archive_member = self
            .active_archive_member(path)
            .map(|(archive, member)| (archive.source_path(), member));
        updater::register_restart(Some(path), archive_member);
    }

    pub(super) fn cache_document(&mut self, path: PathBuf, document: ImageDocument) {
        const MAX_DOCUMENT_CACHE_BYTES: usize = crate::resources::PREFETCH_LIMIT;
        const MAX_TOTAL_CACHE_BYTES: usize = crate::resources::PREFETCH_LIMIT;

        if self.cache.iter().any(|(cached, _)| *cached == path) {
            return;
        }
        let bytes = document_bytes(&document);
        if bytes > MAX_DOCUMENT_CACHE_BYTES {
            return;
        }
        while self.cache.len() >= 8 || self.cache_bytes + bytes > MAX_TOTAL_CACHE_BYTES {
            if self.cache.is_empty() {
                break;
            }
            let evicted = self.cache.remove(0);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&evicted.1));
            drop(evicted);
        }
        if self.cache_bytes + bytes > MAX_TOTAL_CACHE_BYTES {
            return;
        }
        self.cache.push((path, document));
        self.cache_bytes += bytes;
    }
}
