use super::*;

impl AppState {
    pub(super) fn edit<F: FnOnce(&mut ImageDocument)>(&mut self, operation: F) {
        if self.file_busy || self.load_started.is_some() {
            return;
        }
        if let Some(document) = &mut self.document {
            operation(document);
            let (width, height) = document.view_dimensions();
            self.center = (width as f32 / 2.0, height as f32 / 2.0);
            self.zoom_mode = ZoomMode::Fit;
            self.request_view();
            self.update_status(None);
        }
    }

    pub(super) fn crop(&mut self) {
        let Some((l, t, r, b)) = self.crop_selection.take() else {
            return;
        };
        self.edit(|doc| {
            let _ = doc.crop(l, t, r, b);
        });
    }

    pub(super) fn set_zoom(&mut self, mode: ZoomMode) {
        self.zoom_mode = mode;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.schedule_view();
        self.update_status(None);
    }

    pub(super) fn change_zoom(&mut self, factor: f32) {
        self.change_zoom_at(factor, None);
    }

    pub(super) fn change_zoom_at(&mut self, factor: f32, anchor: Option<POINT>) {
        if self.document.is_none() {
            return;
        }
        if !matches!(self.zoom_mode, ZoomMode::Custom) {
            let _ = self.calculate_view();
            self.zoom_mode = ZoomMode::Custom;
        }
        let area = self.image_area();
        let offset = anchor
            .filter(|point| {
                point.x >= area.left
                    && point.x < area.right
                    && point.y >= area.top
                    && point.y < area.bottom
            })
            .map(|point| {
                (
                    point.x as f32 - (area.left + area.right) as f32 / 2.0,
                    point.y as f32 - (area.top + area.bottom) as f32 / 2.0,
                )
            });
        let old_zoom = self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.01, 64.0);
        if let Some((x, y)) = offset {
            let zoom_adjustment = 1.0 / old_zoom - 1.0 / self.zoom;
            self.center.0 += x * zoom_adjustment;
            self.center.1 += y * zoom_adjustment;
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.schedule_view();
        self.update_status(None);
    }

    pub(super) fn finish_selection(&mut self) {
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
        let (width, height) = self.document.as_ref().unwrap().view_dimensions();
        let l = (((start.x.min(end.x) as f32 - destination.left) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(width);
        let t = (((start.y.min(end.y) as f32 - destination.top) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(height);
        let r = (((start.x.max(end.x) as f32 - destination.left) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .min(width);
        let b = (((start.y.max(end.y) as f32 - destination.top) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .min(height);
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
        self.schedule_view();
        self.update_status(None);
    }

    pub(super) fn toggle_fullscreen(&mut self) {
        self.bitmaps.clear();
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

    pub(super) fn toggle_slideshow(&mut self) {
        self.slideshow = !self.slideshow;
        unsafe {
            if self.slideshow {
                if self.auto_advance {
                    self.auto_advance = false;
                    let _ = KillTimer(Some(self.hwnd), TIMER_AUTO_ADVANCE);
                }
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
            self.rebuild_menu();
        }
    }

    pub(super) fn toggle_auto_advance(&mut self) {
        self.auto_advance = !self.auto_advance;
        unsafe {
            if self.auto_advance {
                if self.slideshow {
                    self.slideshow = false;
                    let _ = KillTimer(Some(self.hwnd), TIMER_SLIDESHOW);
                }
                let _ = SetTimer(
                    Some(self.hwnd),
                    TIMER_AUTO_ADVANCE,
                    self.settings.auto_advance_ms,
                    None,
                );
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_AUTO_ADVANCE);
            }
            self.rebuild_menu();
        }
    }

    pub(super) fn set_auto_advance_interval(&mut self, interval_ms: u32) {
        self.settings.auto_advance_ms = interval_ms.clamp(100, 60_000);
        self.settings.save();
        if self.auto_advance && !unsafe { IsIconic(self.hwnd).as_bool() } {
            unsafe {
                let _ = SetTimer(
                    Some(self.hwnd),
                    TIMER_AUTO_ADVANCE,
                    self.settings.auto_advance_ms,
                    None,
                );
            }
        }
        unsafe {
            self.rebuild_menu();
        }
    }

    pub(super) fn set_animation_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ANIMATION);
        }
        let Some(doc) = &self.document else { return };
        if doc.format_name == "GIF"
            && doc.frame_count() > 1
            && self.animation_playing
            && !self.view_pending
            && !unsafe { IsIconic(self.hwnd).as_bool() }
        {
            let delay = doc.current_frame().delay_ms.clamp(20, 60_000);
            unsafe {
                let _ = SetTimer(Some(self.hwnd), TIMER_ANIMATION, delay, None);
            }
        }
    }
}
