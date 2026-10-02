impl AppState {
    fn edit<F: FnOnce(&mut ImageDocument)>(&mut self, operation: F) {
        if let Some(document) = &mut self.document {
            operation(document);
            let frame = document.current_frame();
            self.center = (frame.width as f32 / 2.0, frame.height as f32 / 2.0);
            self.zoom_mode = ZoomMode::Fit;
            self.invalidate_image();
        }
    }

    fn crop(&mut self) {
        let Some((l, t, r, b)) = self.crop_selection.take() else {
            return;
        };
        self.edit(|doc| {
            let _ = doc.crop(l, t, r, b);
        });
    }

    fn set_zoom(&mut self, mode: ZoomMode) {
        self.zoom_mode = mode;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.update_status(None);
    }

    fn change_zoom(&mut self, factor: f32) {
        if self.document.is_none() {
            return;
        }
        if !matches!(self.zoom_mode, ZoomMode::Custom) {
            let _ = self.calculate_view();
            self.zoom_mode = ZoomMode::Custom;
        }
        self.zoom = (self.zoom * factor).clamp(0.01, 64.0);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.update_status(None);
    }

    fn finish_selection(&mut self) {
        let (Some(start), Some(end)) = (self.selection_start.take(), self.selection_end.take())
        else {
            return;
        };
        if (end.x - start.x).abs() < 8 || (end.y - start.y).abs() < 8 {
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
            return;
        }
        let Some(destination) = self.calculate_view() else {
            return;
        };
        let frame = self.document.as_ref().unwrap().current_frame();
        let l = (((start.x.min(end.x) as f32 - destination.left) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(frame.width);
        let t = (((start.y.min(end.y) as f32 - destination.top) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(frame.height);
        let r = (((start.x.max(end.x) as f32 - destination.left) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .min(frame.width);
        let b = (((start.y.max(end.y) as f32 - destination.top) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .min(frame.height);
        if r > l && b > t {
            let area = self.image_area();
            self.zoom = (((area.right - area.left) as f32 / (r - l) as f32)
                .min((area.bottom - area.top) as f32 / (b - t) as f32))
            .clamp(0.01, 64.0);
            self.center = ((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
            self.zoom_mode = ZoomMode::Custom;
            self.crop_selection = Some((l, t, r, b));
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.update_status(None);
    }

    fn toggle_fullscreen(&mut self) {
        self.bitmap = None;
        self.target = None;
        self.chrome.reset_render_resources();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_AUTO_RESIZE);
            if !self.fullscreen {
                let _ = GetWindowRect(self.hwnd, &mut self.saved_rect);
                self.saved_style = WINDOW_STYLE(GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32);
                self.fullscreen = true;
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info).as_bool() {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        info.rcMonitor.left,
                        info.rcMonitor.top,
                        info.rcMonitor.right - info.rcMonitor.left,
                        info.rcMonitor.bottom - info.rcMonitor.top,
                        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_SHOWWINDOW,
                    );
                }
            } else {
                self.fullscreen = false;
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, self.saved_style.0 as isize);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    self.saved_rect.left,
                    self.saved_rect.top,
                    self.saved_rect.right - self.saved_rect.left,
                    self.saved_rect.bottom - self.saved_rect.top,
                    SWP_FRAMECHANGED | SWP_NOZORDER | SWP_SHOWWINDOW,
                );
            }
        }
        self.set_zoom(ZoomMode::Fit);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn toggle_slideshow(&mut self) {
        self.slideshow = !self.slideshow;
        unsafe {
            if self.slideshow {
                let _ = SetTimer(
                    Some(self.hwnd),
                    TIMER_SLIDESHOW,
                    self.settings.slideshow_ms,
                    None,
                );
                if !self.fullscreen {
                    self.toggle_fullscreen();
                }
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_SLIDESHOW);
            }
        }
    }

    fn set_animation_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ANIMATION);
        }
        let Some(doc) = &self.document else { return };
        if doc.format_name == "GIF" && doc.frame_count() > 1 && self.animation_playing {
            let delay = doc.current_frame().delay_ms.clamp(20, 60_000);
            unsafe {
                let _ = SetTimer(Some(self.hwnd), TIMER_ANIMATION, delay, None);
            }
        }
    }
}

