impl AppState {
    fn delete_current(&mut self) {
        let Some(doc) = &self.document else { return };
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
        let path = doc.path.clone();
        let next = self
            .catalog
            .adjacent(1)
            .or_else(|| self.catalog.adjacent(-1));
        let mut from = wide_path(&path);
        from.push(0);
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: self.hwnd,
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO.0 | FOF_NOERRORUI.0) as u16,
            ..Default::default()
        };
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result == 0 && !operation.fAnyOperationsAborted.as_bool() {
            self.generation = self.generation.wrapping_add(1);
            self.load_started = None;
            self.document = None;
            self.bitmap = None;
            self.set_animation_timer();
            self.active_path = None;
            self.failed_paths.remove(&path);
            self.catalog.files.retain(|candidate| !paths_equal(candidate, &path));
            self.catalog_stamp = None;
            self.cache.retain(|(candidate, _)| !paths_equal(candidate, &path));
            self.cache_bytes = self
                .cache
                .iter()
                .map(|(_, document)| document_bytes(document))
                .sum();
            if let Some(next) = next.filter(|candidate| candidate.exists()) {
                self.catalog.update_index(&next);
                self.start_load(next);
            } else {
                self.catalog = FolderCatalog::default();
                self.update_title();
                self.update_status(None);
                self.invalidate_image();
            }
        } else if result != 0 && !operation.fAnyOperationsAborted.as_bool() {
            let error = std::io::Error::from_raw_os_error(result);
            self.show_error(&if self.lang() == Language::Spanish {
                format!("No se pudo enviar la imagen a la Papelera: {error}")
            } else {
                format!("Could not move the image to the Recycle Bin: {error}")
            });
        }
    }

    fn copy_path(&self) {
        if let Some(doc) = &self.document {
            copy_text(self.hwnd, &doc.path.to_string_lossy());
        }
    }

    fn copy_image(&self) {
        let Some(doc) = &self.document else { return };
        let frame = doc.current_frame();
        copy_bgra_image(self.hwnd, frame.width, frame.height, &frame.pixels);
    }

    fn copy_url(&mut self) {
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
            let payload = Box::new(UploadPayload { result });
            let raw = Box::into_raw(payload);
            let hwnd = HWND(hwnd_raw as *mut c_void);
            if unsafe { PostMessageW(Some(hwnd), WM_UPLOAD_DONE, WPARAM(0), LPARAM(raw as isize)) }
                .is_err()
            {
                unsafe {
                    drop(Box::from_raw(raw));
                }
            }
        });
        if let Err(error) = spawn_result {
            self.uploading = false;
            self.update_status(None);
            self.show_error(&format!("No se pudo iniciar la subida: {error}"));
        }
    }

    fn accept_upload(&mut self, payload: UploadPayload) {
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

    fn open_folder(&self) {
        let Some(doc) = &self.document else { return };
        let argument = format!("/select,\"{}\"", doc.path.display());
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
