use super::*;

impl AppState {
    pub(super) unsafe fn create_controls(&mut self) {
        self.file_service.start(self.hwnd);
        self.folder_watcher.start(self.hwnd);
        unsafe {
            self.rebuild_menu();
        }
        unsafe { DragAcceptFiles(self.hwnd, true) };
        unsafe {
            self.apply_theme();
        }
        self.update_status(None);
    }

    pub(super) unsafe fn apply_theme(&self) {
        unsafe { apply_window_theme(self.hwnd, self.settings.is_dark()) };
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }

    pub(super) fn chrome_layout(&self) -> ChromeLayout {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        ChromeLayout::calculate(
            self.hwnd,
            rect,
            self.fullscreen,
            self.update_available.is_some(),
        )
    }

    pub(super) fn keeps_window_size(&self) -> bool {
        self.fullscreen || unsafe { IsZoomed(self.hwnd).as_bool() }
    }

    pub(super) unsafe fn ensure_target(&mut self) -> Result<(), WinError> {
        if self.target.is_none() {
            let mut client = RECT::default();
            unsafe { GetClientRect(self.hwnd, &mut client)? };
            let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: client.right.max(1) as u32,
                    height: client.bottom.max(1) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            // The app's layout and input coordinates use physical client pixels
            // and apply the window DPI scale explicitly. Keep Direct2D in that
            // same coordinate space instead of also scaling through system DPI.
            let render_properties = D2D1_RENDER_TARGET_PROPERTIES {
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            self.target = Some(unsafe {
                self.d2d_factory
                    .CreateHwndRenderTarget(&render_properties, &hwnd_properties)?
            });
        }
        Ok(())
    }

    pub(super) unsafe fn ensure_bitmap(&mut self) -> Result<(), WinError> {
        if self.document.is_none() {
            self.bitmaps.clear();
            return Ok(());
        }
        unsafe {
            self.ensure_target()?;
        }
        let frame = self.document.as_ref().unwrap().current_frame();
        let target = self.target.as_ref().unwrap();
        let tile_size = unsafe { target.GetMaximumBitmapSize() }.clamp(1, 2048);
        let properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let reuse = !self.bitmaps.is_empty() && self.bitmap_size == (frame.width, frame.height);
        if reuse && !self.bitmap_dirty {
            return Ok(());
        }
        let _span = crate::diagnostics::Span::new("gpu_upload");
        if !reuse {
            self.bitmaps.clear();
            for top in (0..frame.height).step_by(tile_size as usize) {
                for left in (0..frame.width).step_by(tile_size as usize) {
                    let width = tile_size.min(frame.width - left);
                    let height = tile_size.min(frame.height - top);
                    let memory = crate::resources::gpu_budget()
                        .reserve(width as usize * height as usize * 4)
                        .map_err(|message| {
                            WinError::new(windows::core::HRESULT(0x8007000Eu32 as i32), message)
                        })?;
                    let offset = (top as usize * frame.width as usize + left as usize) * 4;
                    let bitmap = unsafe {
                        target.CreateBitmap(
                            D2D_SIZE_U { width, height },
                            Some(frame.pixels[offset..].as_ptr().cast()),
                            frame.width * 4,
                            &properties,
                        )?
                    };
                    self.bitmaps.push(BitmapTile {
                        bitmap,
                        left,
                        top,
                        width,
                        height,
                        _memory: memory,
                    });
                }
            }
        } else {
            for tile in &self.bitmaps {
                let offset = (tile.top as usize * frame.width as usize + tile.left as usize) * 4;
                unsafe {
                    tile.bitmap.CopyFromMemory(
                        None,
                        frame.pixels[offset..].as_ptr().cast(),
                        frame.width * 4,
                    )?;
                }
            }
        }
        self.bitmap_size = (frame.width, frame.height);
        self.bitmap_dirty = false;
        Ok(())
    }

    pub(super) fn image_area(&self) -> RECT {
        let image = self.chrome_layout().image;
        RECT {
            left: image.left.round() as i32,
            top: image.top.round() as i32,
            right: image.right.round() as i32,
            bottom: image.bottom.round() as i32,
        }
    }

    pub(super) fn calculate_view(&mut self) -> Option<D2D_RECT_F> {
        let (width, height) = self.document.as_ref()?.view_dimensions();
        let area = self.image_area();
        let aw = (area.right - area.left).max(1) as f32;
        let ah = (area.bottom - area.top).max(1) as f32;
        let iw = width as f32;
        let ih = height as f32;
        self.zoom = match self.zoom_mode {
            ZoomMode::Fit => (aw / iw).min(ah / ih),
            ZoomMode::FitWidth => (aw / iw).max(0.001),
            ZoomMode::Actual => 1.0,
            ZoomMode::Custom => self.zoom.clamp(0.01, 64.0),
        };
        if !matches!(self.zoom_mode, ZoomMode::Custom) {
            self.center = (iw / 2.0, ih / 2.0);
        }
        let cx = (area.left + area.right) as f32 / 2.0;
        let cy = (area.top + area.bottom) as f32 / 2.0;
        Some(D2D_RECT_F {
            left: cx - self.center.0 * self.zoom,
            top: cy - self.center.1 * self.zoom,
            right: cx + (iw - self.center.0) * self.zoom,
            bottom: cy + (ih - self.center.1) * self.zoom,
        })
    }

    pub(super) unsafe fn paint(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
        if unsafe { self.ensure_target() }.is_err() {
            return;
        }
        if let Err(error) = unsafe { self.ensure_bitmap() } {
            eprintln!("KGen View Direct2D bitmap error: {error:?}");
        }
        let palette = Palette::for_dark(self.settings.is_dark());
        let target = self.target.as_ref().unwrap().clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&palette.canvas));
        }
        let destination = self.calculate_view();
        if let (Some(document), Some(destination)) = (self.document.as_ref(), destination) {
            let frame = document.current_frame();
            let (l, t, r, b) = document.rendered_region;
            let sx = (r - l) as f32 / frame.width as f32 * self.zoom;
            let sy = (b - t) as f32 / frame.height as f32 * self.zoom;
            for tile in &self.bitmaps {
                let region = D2D_RECT_F {
                    left: destination.left + l as f32 * self.zoom + tile.left as f32 * sx,
                    top: destination.top + t as f32 * self.zoom + tile.top as f32 * sy,
                    right: destination.left
                        + l as f32 * self.zoom
                        + (tile.left + tile.width) as f32 * sx,
                    bottom: destination.top
                        + t as f32 * self.zoom
                        + (tile.top + tile.height) as f32 * sy,
                };
                unsafe {
                    target.DrawBitmap(
                        &tile.bitmap,
                        Some(&region),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }
        }
        let layout = self.chrome_layout();
        let mut status = self.status_model.clone();
        if let Some(label) = self.chrome.label_for_hover(self.lang()) {
            status.primary = label.to_string();
        }
        let active_command = match self.zoom_mode {
            ZoomMode::Fit => Some(cmd::FIT),
            ZoomMode::FitWidth => Some(cmd::FIT_WIDTH),
            ZoomMode::Actual => Some(cmd::ACTUAL),
            ZoomMode::Custom => None,
        };
        unsafe {
            self.chrome.draw(
                &target,
                &layout,
                palette,
                active_command,
                self.lang(),
                self.settings.is_dark(),
            );
        }
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let rect = D2D_RECT_F {
                left: start.x.min(end.x) as f32,
                top: start.y.min(end.y) as f32,
                right: start.x.max(end.x) as f32,
                bottom: start.y.max(end.y) as f32,
            };
            if let Some(brush) = self.chrome.accent_brush() {
                unsafe { target.DrawRectangle(&rect, brush, 2.0, None) };
            }
        }
        if unsafe { target.EndDraw(None, None) }.is_err() {
            self.target = None;
            self.bitmaps.clear();
            self.chrome.reset_render_resources();
        } else {
            if let Some(started) = self.first_visible.take() {
                crate::diagnostics::elapsed("first_visible", started.elapsed());
            }
            unsafe {
                crate::ui::status::draw(hdc, &layout, &status, self.settings.is_dark());
            }
        }
    }
}
