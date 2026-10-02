impl AppState {
    fn open_dialog(&mut self) {
        let mut buffer = vec![0u16; 32768];
        let filter = wide_with_nuls(
            "Imágenes compatibles\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxl;*.jxr;*.wdp;*.dds\0Todos los archivos\0*.*\0\0",
        );
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: self.hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY,
            ..Default::default()
        };
        if unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..nul_pos(&buffer)]));
            if self.can_leave() {
                self.start_load(path);
            }
        }
    }

    fn save_as_dialog(&mut self) -> bool {
        let Some(document) = &self.document else {
            return false;
        };
        let mut buffer = vec![0u16; 32768];
        let suggested = document
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let suggested_w = wide(&suggested);
        let count = suggested_w.len().min(buffer.len());
        buffer[..count].copy_from_slice(&suggested_w[..count]);
        let filter =
            wide_with_nuls("PNG\0*.png\0JPEG\0*.jpg;*.jpeg\0Bitmap\0*.bmp\0TIFF\0*.tif;*.tiff\0\0");
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: self.hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT,
            lpstrDefExt: w!("png"),
            ..Default::default()
        };
        if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
            return false;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..nul_pos(&buffer)]));
        let result = self.document.as_mut().unwrap().save_as(&path);
        match result {
            Ok(()) => {
                self.catalog = FolderCatalog::scan(&path);
                self.update_title();
                self.update_status(None);
                true
            }
            Err(error) => {
                self.show_error(&error.to_string());
                false
            }
        }
    }

    fn show_error(&self, text: &str) {
        let text = wide(text);
        let caption = wide(tr(self.lang(), Text::Error));
        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                PCWSTR(text.as_ptr()),
                PCWSTR(caption.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn info(&mut self) {
        let Some(doc) = &self.document else { return };
        let frame = doc.current_frame();
        let pixels = u64::from(frame.width) * u64::from(frame.height);
        let unique_colors = if pixels <= 4_000_000 {
            let count = frame
                .pixels
                .chunks_exact(4)
                .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
                .collect::<HashSet<_>>()
                .len();
            count.to_string()
        } else {
            "Más de 4 millones (imagen grande)".into()
        };
        let author = metadata_value(doc, "Autor");
        let description = metadata_value(doc, "Descripción");
        let folder = doc.path.parent().map(display_path).unwrap_or_default();
        let file_name = doc
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let position = self
            .catalog
            .index
            .map(|index| format!("{} / {}", index + 1, self.catalog.files.len()))
            .unwrap_or_else(|| "—".into());
        let ratio = frame.width as f64 / frame.height.max(1) as f64;
        let data = PropertiesData {
            file_name,
            folder,
            full_path: display_path(&doc.path),
            format: format!("{} · WIC/GIF nativo", doc.format_name),
            resolution: "96 × 96 DPI".into(),
            original_size: format!("{} × {} píxeles ({ratio:.2})", frame.width, frame.height),
            current_size: format!("{} × {} píxeles ({ratio:.2})", frame.width, frame.height),
            print_size: format!(
                "{:.2} × {:.2} cm · {:.2} × {:.2} pulgadas",
                frame.width as f64 / 96.0 * 2.54,
                frame.height as f64 / 96.0 * 2.54,
                frame.width as f64 / 96.0,
                frame.height as f64 / 96.0,
            ),
            original_colors: "16,7 millones · 32 bits BGRA".into(),
            current_colors: "16,7 millones · 32 bits BGRA".into(),
            unique_colors,
            disk_size: format!("{} ({} bytes)", format_bytes(doc.file_size), doc.file_size),
            memory_size: format_bytes(frame.pixels.len() as u64),
            folder_index: position,
            file_time: file_timestamp(&doc.path),
            load_time: format!("{} milisegundos", self.load_time_ms),
            author,
            description,
            api_key: self.settings.imgbb_api_key.clone(),
        };
        let Some(edit) =
            crate::platform::properties_dialog::show(self.hwnd, self.settings.is_dark(), data)
        else {
            return;
        };
        self.apply_properties(edit);
    }

    fn apply_properties(&mut self, edit: PropertiesEdit) {
        if self.settings.imgbb_api_key != edit.api_key.trim() {
            self.settings.imgbb_api_key = edit.api_key.trim().into();
            self.settings.save();
        }
        let Some(document) = &mut self.document else {
            return;
        };
        let old_path = document.path.clone();
        let old_name = old_path.file_name().unwrap_or_default().to_string_lossy();
        let new_name = edit.file_name.trim();
        if !new_name.is_empty() && new_name != old_name {
            if new_name.contains(['/', '\\']) {
                self.show_error("El nombre no puede contener separadores de carpeta.");
                return;
            }
            let target = old_path.with_file_name(new_name);
            if target.exists() {
                self.show_error("Ya existe un archivo con ese nombre.");
                return;
            }
            if let Err(error) = fs::rename(&old_path, &target) {
                self.show_error(&format!("No se pudo renombrar el archivo: {error}"));
                return;
            }
            document.path = target.clone();
            self.active_path = Some(target.clone());
            self.catalog = FolderCatalog::scan(&target);
        }
        let changed = set_metadata_value(document, "Autor", edit.author.trim())
            | set_metadata_value(document, "Descripción", edit.description.trim());
        if changed {
            document.dirty = true;
        }
        self.update_title();
        self.update_status(None);
    }
}
