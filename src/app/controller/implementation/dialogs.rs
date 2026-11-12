use super::*;

impl AppState {
    pub(super) fn ensure_export_formats(&mut self) -> bool {
        if self.export_formats.is_none() {
            let formats = ImageDocument::available_export_formats().unwrap_or_default();
            self.selected_export_format = formats
                .iter()
                .position(|format| format.accepts_extension("png"))
                .unwrap_or(0);
            self.export_formats = Some(formats);
        }
        self.export_formats
            .as_ref()
            .is_some_and(|formats| !formats.is_empty())
    }

    pub(super) fn open_dialog(&mut self) {
        let mut buffer = vec![0u16; 32768];
        let filter = wide_with_nuls(if self.lang() == Language::Spanish {
            "Imágenes y archivos ZIP\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxl;*.jxr;*.wdp;*.hdp;*.dds;*.svg;*.zip;*.cbz\0Todos los archivos\0*.*\0\0"
        } else {
            "Images and ZIP archives\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxl;*.jxr;*.wdp;*.hdp;*.dds;*.svg;*.zip;*.cbz\0All files\0*.*\0\0"
        });
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
            self.request_leave(PendingAction::Load(path));
        }
    }

    pub(super) fn save_as_dialog(&mut self) -> bool {
        if self.document.is_none() || self.file_busy {
            return false;
        }
        if !self.ensure_export_formats() {
            self.show_error(if self.lang() == Language::Spanish {
                "Windows no ofrece ningún codificador de imágenes en este equipo."
            } else {
                "Windows does not provide any image encoders on this computer."
            });
            return false;
        }
        let export_formats = self.export_formats.as_ref().unwrap();
        let document = self.document.as_ref().unwrap();
        let mut buffer = vec![0u16; 32768];
        let extension = document
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let default_format = export_formats
            .iter()
            .position(|format| format.accepts_extension(&extension))
            .unwrap_or(self.selected_export_format.min(export_formats.len() - 1));
        let format = export_formats[default_format].clone();
        let source_name = self
            .active_archive_member(&document.path)
            .map(|(_, member)| member.rsplit('/').next().unwrap_or(member).to_owned())
            .unwrap_or_else(|| {
                document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        let suggested = if format.accepts_extension(&extension) {
            source_name.clone()
        } else {
            format!(
                "{}.{}",
                Path::new(&source_name)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy(),
                format.extension()
            )
        };
        let suggested_w = wide(&suggested);
        let count = suggested_w.len().min(buffer.len());
        buffer[..count].copy_from_slice(&suggested_w[..count]);
        let mut filter_parts = Vec::with_capacity(export_formats.len() * 2 + 2);
        for format in export_formats {
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
        let chosen_index = if dialog.nFilterIndex == 0 || filter_index >= export_formats.len() {
            export_formats
                .iter()
                .position(|format| format.accepts_extension(path_extension))
                .unwrap_or(default_format)
        } else {
            filter_index
        };
        let format = export_formats[chosen_index].clone();
        let path_extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !format.accepts_extension(path_extension) {
            path.set_extension(format.extension());
        }
        let document = self.document.as_ref().unwrap().export_snapshot();
        self.file_busy = self.file_service.submit(FileTask::Save {
            document,
            path,
            format,
        });
        if self.file_busy {
            self.update_status(Some(if self.lang() == Language::Spanish {
                "Guardando…"
            } else {
                "Saving…"
            }));
        }
        self.file_busy
    }

    pub(super) fn convert_dialog(&mut self) {
        if unsafe { IsWindow(Some(self.conversion_window)) }.as_bool() {
            unsafe {
                let _ = ShowWindow(self.conversion_window, SW_RESTORE);
                let _ = SetForegroundWindow(self.conversion_window);
            }
            return;
        }
        if self.document.is_none() {
            return;
        }
        if !self.ensure_export_formats() {
            self.show_error(if self.lang() == Language::Spanish {
                "No se encontró ningún formato de salida compatible en este equipo."
            } else {
                "No compatible output formats were found on this computer."
            });
            return;
        }
        let document = self.document.as_ref().unwrap();
        let export_formats = self.export_formats.as_ref().unwrap();
        let window = crate::platform::conversion_dialog::show(
            self.hwnd,
            self.settings.is_dark(),
            self.lang(),
            document,
            export_formats.clone(),
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

    pub(super) fn show_error(&self, text: &str) {
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

    pub(super) fn info(&mut self) {
        if self.file_busy {
            return;
        }
        let Some(doc) = &self.document else { return };
        let spanish = self.lang() == Language::Spanish;
        let frame = doc.current_frame();
        let (width, height) = doc.view_dimensions();
        let pixels = u64::from(width) * u64::from(height);
        let unique_colors = if !doc.is_preview && pixels <= 500_000 {
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
        let archive_source = self
            .active_archive_member(&doc.path)
            .map(|(archive, _)| archive.source_path().to_path_buf());
        let archive_member = self
            .active_archive_member(&doc.path)
            .map(|(_, member)| member.to_owned());
        let folder = archive_source
            .as_deref()
            .and_then(Path::parent)
            .or_else(|| doc.path.parent())
            .map(display_path)
            .unwrap_or_default();
        let file_name = archive_member
            .as_deref()
            .map(|member| member.rsplit('/').next().unwrap_or(member).to_owned())
            .unwrap_or_else(|| {
                doc.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        let position = self
            .image_position()
            .map(|(index, count)| format!("{index} / {count}"))
            .unwrap_or_else(|| "—".into());
        let ratio = width as f64 / height.max(1) as f64;
        let (dpi_x, dpi_y) = doc.resolution_dpi;
        let dpi_x = if dpi_x.is_finite() && dpi_x > 0.0 {
            dpi_x
        } else {
            96.0
        };
        let dpi_y = if dpi_y.is_finite() && dpi_y > 0.0 {
            dpi_y
        } else {
            96.0
        };
        let (original_width, original_height) = doc.original_size;
        let memory_bytes = doc.frames.iter().fold(0u64, |total, frame| {
            total.saturating_add(frame.pixels.len() as u64)
        });
        let data = PropertiesData {
            file_name,
            folder,
            full_path: self.display_file_location(&doc.path),
            format: doc.format_name.clone(),
            resolution: format!("{dpi_x:.2} × {dpi_y:.2} DPI"),
            original_size: if spanish {
                format!("{original_width} × {original_height} píxeles")
            } else {
                format!("{original_width} × {original_height} pixels")
            },
            current_size: if spanish {
                format!("{} × {} píxeles ({ratio:.2})", width, height)
            } else {
                format!("{} × {} pixels ({ratio:.2})", width, height)
            },
            print_size: if spanish {
                format!(
                    "{:.2} × {:.2} cm · {:.2} × {:.2} pulgadas",
                    width as f64 / dpi_x * 2.54,
                    height as f64 / dpi_y * 2.54,
                    width as f64 / dpi_x,
                    height as f64 / dpi_y,
                )
            } else {
                format!(
                    "{:.2} × {:.2} cm · {:.2} × {:.2} inches",
                    width as f64 / dpi_x * 2.54,
                    height as f64 / dpi_y * 2.54,
                    width as f64 / dpi_x,
                    height as f64 / dpi_y,
                )
            },
            current_colors: "32 bits BGRA".into(),
            unique_colors,
            disk_size: format!("{} ({} bytes)", format_bytes(doc.file_size), doc.file_size),
            memory_size: format_bytes(memory_bytes),
            folder_index: position,
            file_time: file_timestamp(archive_source.as_deref().unwrap_or(&doc.path)),
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
        ) else {
            return;
        };
        self.apply_properties(edit);
    }

    pub(super) fn apply_properties(&mut self, edit: PropertiesEdit) {
        let spanish = self.lang() == Language::Spanish;
        let Some(old_path) = self.document.as_ref().map(|document| document.path.clone()) else {
            return;
        };
        let old_name = self
            .active_archive_member(&old_path)
            .map(|(_, member)| member.rsplit('/').next().unwrap_or(member).to_owned())
            .unwrap_or_else(|| {
                old_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        let new_name = edit.file_name.trim();
        if !new_name.is_empty() && new_name != old_name.as_str() {
            if self.active_archive_member(&old_path).is_some() {
                self.show_error(if spanish {
                    "No se puede renombrar una imagen dentro de un ZIP. Usa Guardar como para crear una copia."
                } else {
                    "An image inside a ZIP archive cannot be renamed. Use Save As to create a copy."
                });
                return;
            }
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
            let new_extension = Path::new(new_name)
                .extension()
                .map(|value| value.to_string_lossy());
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
            let id = self.document.as_ref().unwrap().id;
            self.file_busy = self.file_service.submit(FileTask::Rename {
                id,
                from: old_path,
                to: target,
            });
            if self.file_busy {
                self.pending_properties = Some(edit);
                self.update_status(Some(if spanish {
                    "Renombrando…"
                } else {
                    "Renaming…"
                }));
            }
            return;
        }
        self.finish_properties(edit);
    }

    pub(super) fn finish_properties(&mut self, edit: PropertiesEdit) {
        if self.settings.imgbb_api_key != edit.api_key.trim() {
            self.settings.imgbb_api_key = edit.api_key.trim().into();
            self.settings.save();
        }
        let Some(document) = &mut self.document else {
            return;
        };
        let changed = set_metadata_value(document, "Autor", edit.author.trim())
            | set_metadata_value(document, "Descripción", edit.description.trim());
        if changed {
            document.dirty = true;
            document.metadata_dirty = true;
            document.revision = document.revision.wrapping_add(1);
        }
        self.update_title();
        self.update_status(None);
    }
}
