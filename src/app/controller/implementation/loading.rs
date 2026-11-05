impl AppState {
    fn start_load(&mut self, path: PathBuf) {
        // Catalog entries have already been normalized by the initial scan.
        // External paths still need canonicalization, even when a catalog is
        // present for a different folder.
        let path = if self.catalog.files.iter().any(|known| known == &path) {
            path
        } else {
            path.canonicalize().unwrap_or(path)
        };
        self.generation = self.generation.wrapping_add(1);
        self.load_started = Some(Instant::now());
        let generation = self.generation;
        // Invalidate active/queued background work even when this target is
        // already cached and no replacement decode or catalog scan is needed.
        self.loader.invalidate(generation);
        self.catalog_loader.invalidate(generation);
        self.pending_catalog = None;

        let is_same_folder =
            self.catalog.files.first().and_then(|known| known.parent()) == path.parent();
        if self.catalog.files.is_empty() || !is_same_folder {
            self.pending_catalog = Some((generation, path.clone()));
            self.catalog_loader
                .submit(generation, path.clone(), self.hwnd);
        }

        let cached_index = self.cache.iter().position(|(cached, _)| *cached == path);
        let cached_document = cached_index.map(|index| {
            let (_, document) = self.cache.remove(index);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&document));
            document
        });
        if let Some(document) = cached_document {
            self.load_time_ms = 0;
            self.load_started = None;
            let mut catalog = self.catalog.clone();
            let is_same_folder = catalog.files.first().and_then(|p| p.parent()) == path.parent();
            if catalog.files.is_empty() || !is_same_folder {
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
                });
            }
            return;
        }

        self.update_status(Some(tr(self.lang(), Text::Loading)));
        self.loader.submit(generation, path, self.hwnd);
    }

    unsafe fn accept_load(&mut self, payload: LoadPayload) {
        if payload.generation != self.generation {
            return;
        }
        match payload.result {
            Ok(document) => {
                if self.load_started.is_some() {
                    self.load_time_ms = self
                        .load_started
                        .take()
                        .map(|started| started.elapsed().as_millis())
                        .unwrap_or(0);
                }
                let is_same_folder =
                    self.catalog.files.first().and_then(|p| p.parent()) == payload.path.parent();
                let needs_catalog = payload.catalog.files.is_empty() && !is_same_folder;
                if payload.catalog.files.is_empty() || is_same_folder {
                    if is_same_folder {
                        self.catalog.update_index(&payload.path);
                    } else {
                        self.catalog = FolderCatalog::default();
                        self.catalog_stamp = None;
                    }
                } else {
                    self.catalog = payload.catalog;
                    self.catalog_stamp = folder_modified(&payload.path);
                }
                self.failed_paths.remove(&payload.path);
                self.active_path = Some(payload.path.clone());
                let frame = document.current_frame();
                self.center = (frame.width as f32 / 2.0, frame.height as f32 / 2.0);
                self.document = Some(document);
                self.zoom_mode = ZoomMode::Fit;
                self.crop_selection = None;
                self.bitmap = None;
                self.set_animation_timer();
                self.update_title();
                if self.settings.auto_resize && !self.keeps_window_size() {
                    self.schedule_auto_resize();
                }
                self.invalidate_image();
                if needs_catalog {
                    let catalog_already_requested =
                        self.pending_catalog
                            .as_ref()
                            .is_some_and(|(generation, path)| {
                                *generation == self.generation && path == &payload.path
                            });
                    if !catalog_already_requested {
                        self.pending_catalog = Some((self.generation, payload.path.clone()));
                        self.catalog_loader.submit(
                            self.generation,
                            payload.path.clone(),
                            self.hwnd,
                        );
                    }
                } else if self.prefetch_enabled {
                    self.start_prefetch();
                }
            }
            Err(error) => {
                if let Some(started) = self.load_started.take() {
                    self.load_time_ms = started.elapsed().as_millis();
                }
                let is_same_folder =
                    self.catalog.files.first().and_then(|p| p.parent()) == payload.path.parent();
                if payload.catalog.files.is_empty() && is_same_folder {
                    self.catalog.update_index(&payload.path);
                } else {
                    self.catalog = payload.catalog;
                    self.catalog_stamp = folder_modified(&payload.path);
                }
                self.failed_paths.insert(payload.path.clone());
                let extension = payload
                    .path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let suffix = if matches!(extension.as_str(), "webp" | "heic" | "heif") {
                    format!("\n\n{}", tr(self.lang(), Text::CodecMissing))
                } else {
                    String::new()
                };
                let next = (1..=self.catalog.files.len()).find_map(|offset| {
                    self.catalog
                        .adjacent_wrapped(offset as isize)
                        .filter(|candidate| !self.failed_paths.contains(candidate))
                });
                if let Some(next) = next {
                    self.update_status(Some(if self.lang() == Language::Spanish {
                        "Imagen dañada o incompatible; pasando a la siguiente…"
                    } else {
                        "Damaged or unsupported image; skipping to the next…"
                    }));
                    self.start_load(next);
                } else {
                    self.update_status(Some(tr(self.lang(), Text::Unsupported)));
                    self.show_error(&format!("{}{}", error, suffix));
                }
            }
        }
    }

    fn start_prefetch(&self) {
        // Keep one bounded adjacent image ready so normal navigation is warm
        // from the first open; speculative work runs on its own worker.
        let offset = if self.navigation_direction < 0 { -1 } else { 1 };
        let offsets = [offset];
        let candidates: Vec<PathBuf> = offsets
            .into_iter()
            .filter_map(|offset| self.catalog.adjacent_wrapped(offset))
            .filter(|path| self.active_path.as_ref() != Some(path))
            .filter(|path| !self.cache.iter().any(|(cached, _)| cached == path))
            .collect();
        self.loader
            .submit_prefetch(self.generation, candidates, self.hwnd);
    }

    fn accept_prefetch(&mut self, payload: PrefetchPayload) {
        if payload.generation != self.generation
            || self.cache.iter().any(|(path, _)| *path == payload.path)
        {
            return;
        }
        let Ok(document) = payload.result else { return };
        self.cache_document(payload.path, document);
    }

    fn cache_document(&mut self, path: PathBuf, document: ImageDocument) {
        const MAX_DOCUMENT_CACHE_BYTES: usize = 32 * 1024 * 1024;
        const MAX_TOTAL_CACHE_BYTES: usize = 32 * 1024 * 1024;

        if self.cache.iter().any(|(cached, _)| *cached == path) {
            return;
        }
        let bytes = document_bytes(&document);
        if bytes > MAX_DOCUMENT_CACHE_BYTES {
            return;
        }
        while self.cache_bytes + bytes > MAX_TOTAL_CACHE_BYTES {
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
