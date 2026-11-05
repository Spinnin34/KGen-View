impl AppState {
    fn open_dialog(&mut self) {
        let mut buffer = vec![0u16; 32768];
        let filter = wide_with_nuls(
            if self.lang() == Language::Spanish {
                "Imágenes compatibles\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxl;*.jxr;*.wdp;*.hdp;*.dds;*.svg\0Todos los archivos\0*.*\0\0"
            } else {
                "Supported images\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxl;*.jxr;*.wdp;*.hdp;*.dds;*.svg\0All files\0*.*\0\0"
            },
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
        if self.export_formats.is_empty() {
            self.show_error(if self.lang() == Language::Spanish {
                "Windows no ofrece ningún codificador de imágenes en este equipo."
            } else {
                "Windows does not provide any image encoders on this computer."
            });
            return false;
        }
        let mut buffer = vec![0u16; 32768];
        let extension = document
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let default_format = self
            .export_formats
            .iter()
            .position(|format| format.accepts_extension(&extension))
            .unwrap_or(self.selected_export_format.min(self.export_formats.len() - 1));
        let format = &self.export_formats[default_format];
        let suggested = if format.accepts_extension(&extension) {
            document
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            format!(
                "{}.{}",
                document
                    .path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy(),
                format.extension()
            )
        };
        let suggested_w = wide(&suggested);
        let count = suggested_w.len().min(buffer.len());
        buffer[..count].copy_from_slice(&suggested_w[..count]);
        let mut filter_parts = Vec::with_capacity(self.export_formats.len() * 2 + 2);
        for format in &self.export_formats {
            filter_parts.push(format.filter_label());
            filter_parts.push(format.filter_pattern());
        }
        filter_parts.push(if self.lang() == Language::Spanish {
            "Todos los archivos".into()
        } else {
            "All files".into()
        });
        filter_parts.push("*.*".into());
        let filter = wide_with_nuls(&filter_parts.join("\0"));
        let default_extension = wide(format.extension());
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: self.hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT,
            nFilterIndex: default_format as u32 + 1,
            lpstrDefExt: PCWSTR(default_extension.as_ptr()),
            ..Default::default()
        };
        if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
            return false;
        }
        let mut path = PathBuf::from(String::from_utf16_lossy(&buffer[..nul_pos(&buffer)]));
        let filter_index = dialog.nFilterIndex.saturating_sub(1) as usize;
        let path_extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let chosen_index = if dialog.nFilterIndex == 0 || filter_index >= self.export_formats.len() {
            self.export_formats
                .iter()
                .position(|format| format.accepts_extension(path_extension))
                .unwrap_or(default_format)
        } else {
            filter_index
        };
        let format = &self.export_formats[chosen_index];
        let path_extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !format.accepts_extension(path_extension) {
            path.set_extension(format.extension());
        }
        let result = self
            .document
            .as_mut()
            .unwrap()
            .save_as_format(&path, format);
        match result {
            Ok(()) => {
                self.active_path = Some(path.clone());
                self.catalog = FolderCatalog::default();
                self.catalog_stamp = None;
                self.catalog_loader
                    .submit(self.generation, path, self.hwnd);
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

    fn convert_dialog(&mut self) {
        if unsafe { IsWindow(Some(self.conversion_window)) }.as_bool() {
            unsafe {
                let _ = ShowWindow(self.conversion_window, SW_RESTORE);
                let _ = SetForegroundWindow(self.conversion_window);
            }
            return;
        }
        let Some(document) = &self.document else {
            return;
        };
        if self.export_formats.is_empty() {
            self.show_error(if self.lang() == Language::Spanish {
                "No se encontró ningún formato de salida compatible en este equipo."
            } else {
                "No compatible output formats were found on this computer."
            });
            return;
        }
        let window = crate::platform::conversion_dialog::show(
            self.hwnd,
            self.settings.is_dark(),
            self.lang(),
            document,
            self.export_formats.clone(),
            self.selected_export_format,
        );
        if let Some(window) = window {
            self.conversion_window = window;
        } else {
            self.show_error(if self.lang() == Language::Spanish {
                "No se pudo abrir la ventana de conversión."
            } else {
                "Could not open the conversion window."
            });
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
        let spanish = self.lang() == Language::Spanish;
        let frame = doc.current_frame();
        let pixels = u64::from(frame.width) * u64::from(frame.height);
        let unique_colors = if pixels <= 500_000 {
            let count = frame
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| u32::from_le_bytes(*pixel))
                .collect::<HashSet<_>>()
                .len();
            count.to_string()
        } else if spanish {
            "No calculado (imagen grande)".into()
        } else {
            "Not calculated (large image)".into()
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
        let (dpi_x, dpi_y) = doc.resolution_dpi;
        let dpi_x = if dpi_x.is_finite() && dpi_x > 0.0 { dpi_x } else { 96.0 };
        let dpi_y = if dpi_y.is_finite() && dpi_y > 0.0 { dpi_y } else { 96.0 };
        let (original_width, original_height) = doc.original_size;
        let memory_bytes = doc
            .frames
            .iter()
            .fold(0u64, |total, frame| total.saturating_add(frame.pixels.len() as u64));
        let data = PropertiesData {
            file_name,
            folder,
            full_path: display_path(&doc.path),
            format: doc.format_name.clone(),
            resolution: format!("{dpi_x:.2} × {dpi_y:.2} DPI"),
            original_size: if spanish {
                format!("{original_width} × {original_height} píxeles")
            } else {
                format!("{original_width} × {original_height} pixels")
            },
            current_size: if spanish {
                format!("{} × {} píxeles ({ratio:.2})", frame.width, frame.height)
            } else {
                format!("{} × {} pixels ({ratio:.2})", frame.width, frame.height)
            },
            print_size: if spanish {
                format!(
                    "{:.2} × {:.2} cm · {:.2} × {:.2} pulgadas"
                    ,
                    frame.width as f64 / dpi_x * 2.54,
                    frame.height as f64 / dpi_y * 2.54,
                    frame.width as f64 / dpi_x,
                    frame.height as f64 / dpi_y,
                )
            } else {
                format!(
                    "{:.2} × {:.2} cm · {:.2} × {:.2} inches"
                    ,
                    frame.width as f64 / dpi_x * 2.54,
                    frame.height as f64 / dpi_y * 2.54,
                    frame.width as f64 / dpi_x,
                    frame.height as f64 / dpi_y,
                )
            },
            current_colors: "32 bits BGRA".into(),
            unique_colors,
            disk_size: format!("{} ({} bytes)", format_bytes(doc.file_size), doc.file_size),
            memory_size: format_bytes(memory_bytes),
            folder_index: position,
            file_time: file_timestamp(&doc.path),
            load_time: if spanish {
                format!("{} milisegundos", self.load_time_ms)
            } else {
                format!("{} milliseconds", self.load_time_ms)
            },
            author,
            description,
            api_key: self.settings.imgbb_api_key.clone(),
        };
        let Some(edit) = crate::platform::properties_dialog::show(
            self.hwnd,
            self.settings.is_dark(),
            self.lang(),
            data,
        )
        else {
            return;
        };
        self.apply_properties(edit);
    }

    fn apply_properties(&mut self, edit: PropertiesEdit) {
        let spanish = self.lang() == Language::Spanish;
        let Some(old_path) = self.document.as_ref().map(|document| document.path.clone()) else {
            return;
        };
        let old_name = old_path.file_name().unwrap_or_default().to_string_lossy();
        let new_name = edit.file_name.trim();
        let mut renamed_path = None;
        if !new_name.is_empty() && new_name != old_name {
            if new_name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
                || new_name.chars().any(char::is_control)
                || new_name.ends_with(['.', ' '])
            {
                self.show_error(if spanish {
                    "El nombre contiene caracteres no válidos."
                } else {
                    "The file name contains invalid characters."
                });
                return;
            }
            let old_extension = old_path.extension().map(|value| value.to_string_lossy());
            let new_extension = Path::new(new_name).extension().map(|value| value.to_string_lossy());
            let same_extension = match (old_extension, new_extension) {
                (Some(old), Some(new)) => old.eq_ignore_ascii_case(&new),
                (None, None) => true,
                _ => false,
            };
            if !same_extension {
                self.show_error(if spanish {
                    "El renombrado no cambia el formato. Usa Guardar como para convertir la imagen."
                } else {
                    "Renaming does not convert the image. Use Save As to change its format."
                });
                return;
            }
            let target = old_path.with_file_name(new_name);
            if target.exists() && !paths_equal(&target, &old_path) {
                self.show_error(if spanish {
                    "Ya existe un archivo con ese nombre."
                } else {
                    "A file with that name already exists."
                });
                return;
            }
            if let Err(error) = fs::rename(&old_path, &target) {
                self.show_error(&if spanish {
                    format!("No se pudo renombrar el archivo: {error}")
                } else {
                    format!("Could not rename the file: {error}")
                });
                return;
            }
            renamed_path = Some(target);
        }

        if self.settings.imgbb_api_key != edit.api_key.trim() {
            self.settings.imgbb_api_key = edit.api_key.trim().into();
            self.settings.save();
        }
        if let Some(target) = renamed_path {
            if let Some(document) = &mut self.document {
                document.path = target.clone();
            }
            self.active_path = Some(target.clone());
            self.catalog = FolderCatalog::default();
            self.catalog_stamp = None;
            self.catalog_loader
                .submit(self.generation, target, self.hwnd);
        }
        let Some(document) = &mut self.document else {
            return;
        };
        let changed = set_metadata_value(document, "Autor", edit.author.trim())
            | set_metadata_value(document, "Descripción", edit.description.trim());
        if changed {
            document.dirty = true;
            document.metadata_dirty = true;
        }
        self.update_title();
        self.update_status(None);
    }
}
