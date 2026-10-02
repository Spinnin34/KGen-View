impl AppState {
    fn start_load(&mut self, path: PathBuf) {
        // Catalog entries are already absolute after the first load. Avoid a
        // filesystem canonicalization on every wheel event; normalize only
        // the initial load and external relative paths.
        let path = if path.is_absolute() && !self.catalog.files.is_empty() {
            path
        } else {
            path.canonicalize().unwrap_or(path)
        };
        if !path.is_file() {
            return;
        }
        self.active_path = Some(path.clone());
        self.generation = self.generation.wrapping_add(1);
        self.load_started = Some(Instant::now());
        let generation = self.generation;

        if let Some(index) = self.cache.iter().position(|(cached, _)| *cached == path) {
            let (_, document) = self.cache.remove(index);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&document));
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
                    self.catalog_loader
                        .submit(self.generation, payload.path.clone(), self.hwnd);
                } else if self.prefetch_enabled {
                    self.start_prefetch();
                }
            }
            Err(error) => {
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
        // Do not spend CPU preloading while the viewer is simply idle on its
        // first image. Once the user starts navigating, warm only the next
        // image in the current direction.
        let offset = if self.navigation_direction < 0 { -1 } else { 1 };
        let offsets = [offset];
        let candidates: Vec<PathBuf> = offsets
            .into_iter()
            .filter_map(|offset| self.catalog.adjacent_wrapped(offset))
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
        let bytes = document_bytes(&document);
        if bytes > 32 * 1024 * 1024 {
            return;
        }
        let max_cache_bytes = 64 * 1024 * 1024;
        while self.cache_bytes + bytes > max_cache_bytes {
            if self.cache.is_empty() {
                break;
            }
            let evicted = self.cache.remove(0);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&evicted.1));
            drop(evicted);
        }
        if self.cache_bytes + bytes > max_cache_bytes {
            return;
        }
        self.cache.push((payload.path, document));
        self.cache_bytes += bytes;
    }
}
