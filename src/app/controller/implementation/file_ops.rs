use super::*;

impl AppState {
    pub(super) fn delete_current(&mut self) {
        if self.file_busy {
            return;
        }
        let Some(document) = &self.document else {
            return;
        };
        if self.active_archive_member(&document.path).is_some() {
            self.show_error(if self.lang() == Language::Spanish {
                "No se pueden borrar archivos directamente dentro de un ZIP."
            } else {
                "Files inside a ZIP cannot be deleted directly."
            });
            return;
        }
        if self.settings.confirm_delete {
            let text = wide(tr(self.lang(), Text::ConfirmDelete));
            let caption = wide(tr(self.lang(), Text::AppTitle));
            if unsafe {
                MessageBoxW(
                    Some(self.hwnd),
                    PCWSTR(text.as_ptr()),
                    PCWSTR(caption.as_ptr()),
                    MB_YESNO | MB_ICONQUESTION,
                )
            }
            .0 != 6
            {
                return;
            }
        }
        let path = document.path.clone();
        self.file_busy = self.file_service.submit(FileTask::Delete(path));
        if self.file_busy {
            self.update_status(Some(if self.lang() == Language::Spanish {
                "Enviando a la Papelera…"
            } else {
                "Moving to the Recycle Bin…"
            }));
        }
    }

    pub(super) fn accept_file_result(&mut self, result: FileResult) {
        self.file_busy = false;
        let mut success = true;
        match result {
            FileResult::Saved {
                id,
                revision,
                result,
            } => match result {
                Ok(document) => {
                    if self
                        .document
                        .as_ref()
                        .is_some_and(|current| current.id == id && current.revision == revision)
                    {
                        self.generation = self.generation.wrapping_add(1);
                        self.loader.invalidate(self.generation);
                        self.catalog_loader.invalidate(self.generation);
                        self.view_pending = false;
                        self.view_key = None;
                        self.requested_page = 0;
                        self.active_path = Some(document.path.clone());
                        self.catalog = FolderCatalog::default();
                        self.catalog_stamp = None;
                        self.active_archive = None;
                        self.cache.clear();
                        self.cache_bytes = 0;
                        self.catalog_loader.submit(
                            self.generation,
                            document.path.clone(),
                            self.hwnd,
                        );
                        self.folder_watcher
                            .watch(document.path.parent().map(Path::to_path_buf));
                        self.document = Some(*document);
                        self.set_animation_timer();
                        self.retain_active_archive_session();
                        self.refresh_restart_registration();
                        self.invalidate_image();
                        self.update_title();
                        self.schedule_view();
                    }
                }
                Err(error) => {
                    success = false;
                    self.show_error(&error);
                }
            },
            FileResult::Exported { path, result } => match result {
                Ok(()) => {
                    self.complete_mcp_export(Ok(path));
                }
                Err(error) => {
                    success = false;
                    self.complete_mcp_export(Err(error));
                }
            },
            FileResult::Renamed { id, path, result } => match result {
                Ok(()) => {
                    if let Some(document) = &mut self.document
                        && document.id == id
                    {
                        document.relocate_source(path.clone());
                        self.active_path = Some(path.clone());
                        self.generation = self.generation.wrapping_add(1);
                        self.loader.invalidate(self.generation);
                        self.catalog_loader.invalidate(self.generation);
                        self.view_key = None;
                        self.view_pending = false;
                        self.cache.clear();
                        self.cache_bytes = 0;
                        self.catalog = FolderCatalog::default();
                        self.catalog_stamp = None;
                        self.catalog_loader
                            .submit(self.generation, path.clone(), self.hwnd);
                        if let Some(edit) = self.pending_properties.take() {
                            self.finish_properties(edit);
                        }
                        self.schedule_view();
                        self.update_title();
                    }
                }
                Err(error) => {
                    success = false;
                    self.pending_properties = None;
                    self.show_error(&error);
                }
            },
            FileResult::Copied(result) => match result.and_then(|image| image.publish(self.hwnd)) {
                Ok(()) => {}
                Err(error) => {
                    success = false;
                    self.show_error(&error);
                }
            },
            FileResult::Deleted { path, result } => match result {
                Ok(true) => {
                    let index = self.catalog.index.unwrap_or(0);
                    self.catalog.remove(&path);
                    self.document = None;
                    self.bitmaps.clear();
                    self.active_path = None;
                    self.failed_paths.remove(&path);
                    self.cache.clear();
                    self.cache_bytes = 0;
                    self.generation = self.generation.wrapping_add(1);
                    self.loader.invalidate(self.generation);
                    self.catalog_loader.invalidate(self.generation);
                    self.view_pending = false;
                    self.view_key = None;
                    self.load_started = None;
                    self.set_animation_timer();
                    if self.pending_action.is_none() && !self.catalog.files.is_empty() {
                        let next =
                            self.catalog.files[index.min(self.catalog.files.len() - 1)].clone();
                        self.start_load(next);
                    } else {
                        self.update_title();
                        self.invalidate_image();
                    }
                }
                Ok(false) => {
                    success = false;
                }
                Err(error) => {
                    success = false;
                    self.show_error(&error);
                }
            },
        }
        self.update_status(None);
        if success {
            if let Some(action) = self.pending_action.take() {
                self.request_leave(action);
            }
        } else {
            self.pending_action = None;
        }
    }

    pub(super) fn copy_path(&self) {
        if let Some(doc) = &self.document {
            copy_text(self.hwnd, &self.display_file_location(&doc.path));
        }
    }

    pub(super) fn copy_image(&mut self) {
        if self.file_busy {
            return;
        }
        if let Some(document) = &self.document {
            self.file_busy = self
                .file_service
                .submit(FileTask::Copy(document.export_snapshot()));
            if self.file_busy {
                self.update_status(Some(if self.lang() == Language::Spanish {
                    "Preparando el portapapeles…"
                } else {
                    "Preparing clipboard…"
                }));
            }
        }
    }

    pub(super) fn copy_url(&mut self) {
        if self.uploading {
            self.update_status(Some("La imagen ya se está subiendo a ImgBB…"));
            return;
        }
        if self.settings.imgbb_api_key.trim().is_empty() {
            self.info();
            if self.settings.imgbb_api_key.trim().is_empty() {
                self.show_error(
                    "Introduce tu clave API de ImgBB en Información y pulsa Guardar. La clave se obtiene gratuitamente en api.imgbb.com.",
                );
                return;
            }
        }
        let Some(document) = &self.document else {
            return;
        };
        let path = document.path.clone();
        let api_key = self.settings.imgbb_api_key.clone();
        let alive = self.alive.clone();
        let upload_result = self.upload_result.clone();
        let _source_lease = document.source_lease.clone();
        let hwnd_raw = self.hwnd.0 as usize;
        self.uploading = true;
        self.update_status(Some("Subiendo a ImgBB…"));
        let spawn_result = thread::Builder::new()
            .name("kgen-imgbb-upload".into())
            .spawn(move || {
                let result = imgbb::upload(&path, &api_key);
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                let _source_lease = _source_lease;
                if upload_result.publish(UploadPayload { result }) {
                    let _ = unsafe {
                        PostMessageW(
                            Some(HWND(hwnd_raw as *mut c_void)),
                            WM_UPLOAD_DONE,
                            WPARAM(0),
                            LPARAM(0),
                        )
                    };
                }
            });
        if let Err(error) = spawn_result {
            self.uploading = false;
            self.update_status(None);
            self.show_error(&format!("No se pudo iniciar la subida: {error}"));
        }
    }

    pub(super) fn accept_upload(&mut self, payload: UploadPayload) {
        self.uploading = false;
        match payload.result {
            Ok(url) => {
                copy_text(self.hwnd, &url);
                self.update_status(Some("Enlace directo copiado al portapapeles"));
            }
            Err(error) => {
                self.update_status(None);
                self.show_error(&error);
            }
        }
    }

    pub(super) fn open_folder(&self) {
        let Some(doc) = &self.document else { return };
        let path = self
            .active_archive_member(&doc.path)
            .map(|(archive, _)| archive.source_path())
            .unwrap_or(&doc.path);
        let argument = format!("/select,\"{}\"", path.display());
        let argument = wide(&argument);
        unsafe {
            ShellExecuteW(
                Some(self.hwnd),
                w!("open"),
                w!("explorer.exe"),
                PCWSTR(argument.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}
