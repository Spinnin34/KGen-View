impl AppState {
    fn invalidate_image(&mut self) {
        self.bitmap = None;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.update_status(None);
    }

    fn update_status(&mut self, temporary: Option<&str>) {
        self.status_model = if let Some(value) = temporary {
            StatusModel::message(value)
        } else if let Some(document) = &self.document {
            let frame = document.current_frame();
            let folder = self
                .catalog
                .index
                .map(|i| format!("{}/{}", i + 1, self.catalog.files.len()))
                .unwrap_or_else(|| "—".into());
            let disk_size = crate::platform::text::format_bytes_decimal(document.file_size);
            let ram_size = crate::platform::text::format_bytes_decimal(frame.pixels.len() as u64);
            let file_time = file_timestamp_short(&document.path);
            StatusModel {
                primary: format!("{} x {} x 24 BPP", frame.width, frame.height),
                position: Some(folder),
                zoom: Some(format!("{:.0} %", self.zoom * 100.0)),
                size: Some(format!("{} / {}", disk_size, ram_size)),
                date: Some(file_time),
                message: None,
            }
        } else {
            StatusModel::message(tr(self.lang(), Text::NoImage))
        };
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

