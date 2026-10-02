impl AppState {
    unsafe fn create_controls(&mut self) {
        unsafe {
            self.rebuild_menu();
        }
        unsafe { DragAcceptFiles(self.hwnd, true) };
        unsafe {
            let _ = SetTimer(
                Some(self.hwnd),
                TIMER_FOLDER_REFRESH,
                FOLDER_REFRESH_INTERVAL_MS,
                None,
            );
        }
        unsafe {
            self.apply_theme();
        }
        self.update_status(None);
    }

    unsafe fn apply_theme(&self) {
        unsafe { apply_window_theme(self.hwnd, self.settings.is_dark()) };
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }

    fn chrome_layout(&self) -> ChromeLayout {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        ChromeLayout::calculate(self.hwnd, rect, self.fullscreen)
    }

    fn keeps_window_size(&self) -> bool {
        self.fullscreen || unsafe { IsZoomed(self.hwnd).as_bool() }
    }

    unsafe fn ensure_target(&mut self) -> Result<(), WinError> {
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
            self.target = Some(unsafe {
                self.d2d_factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES::default(),
                    &hwnd_properties,
                )?
            });
        }
        Ok(())
    }

    unsafe fn ensure_bitmap(&mut self) -> Result<(), WinError> {
        if self.bitmap.is_some() || self.document.is_none() {
            return Ok(());
        }
        unsafe { self.ensure_target()? };
        let frame = self.document.as_ref().unwrap().current_frame();
        let properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        self.bitmap = Some(unsafe {
            self.target.as_ref().unwrap().CreateBitmap(
                D2D_SIZE_U {
                    width: frame.width,
                    height: frame.height,
                },
                Some(frame.pixels.as_ptr().cast()),
                frame.width * 4,
                &properties,
            )?
        });
        Ok(())
    }

    fn image_area(&self) -> RECT {
        let image = self.chrome_layout().image;
        RECT {
            left: image.left.round() as i32,
            top: image.top.round() as i32,
            right: image.right.round() as i32,
            bottom: image.bottom.round() as i32,
        }
    }

    fn calculate_view(&mut self) -> Option<D2D_RECT_F> {
        let frame = self.document.as_ref()?.current_frame();
        let area = self.image_area();
        let aw = (area.right - area.left).max(1) as f32;
        let ah = (area.bottom - area.top).max(1) as f32;
        let iw = frame.width as f32;
        let ih = frame.height as f32;
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

    unsafe fn paint(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
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
        if let (Some(bitmap), Some(destination)) = (self.bitmap.as_ref(), destination) {
            unsafe {
                target.DrawBitmap(
                    bitmap,
                    Some(&destination),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
        }
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let rect = D2D_RECT_F {
                left: start.x.min(end.x) as f32,
                top: start.y.min(end.y) as f32,
                right: start.x.max(end.x) as f32,
                bottom: start.y.max(end.y) as f32,
            };
            let color = D2D1_COLOR_F {
                r: 1.0,
                g: 0.54,
                b: 0.16,
                a: 1.0,
            };
            if let Ok(brush) = unsafe { target.CreateSolidColorBrush(&color, None) } {
                unsafe { target.DrawRectangle(&rect, &brush, 2.0, None) };
            }
        }
        let layout = self.chrome_layout();
        let mut status = self.status_model.clone();
        if let Some(label) = self.chrome.label_for_hover() {
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
        if unsafe { target.EndDraw(None, None) }.is_err() {
            self.target = None;
            self.bitmap = None;
            self.chrome.reset_render_resources();
        } else {
            unsafe {
                crate::ui::status::draw(hdc, &layout, &status, self.settings.is_dark());
            }
        }
    }
}

