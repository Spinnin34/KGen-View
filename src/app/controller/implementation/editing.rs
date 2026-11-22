use super::*;
use crate::platform::image_tools::{self, SizeTool};

impl AppState {
    pub(super) fn open_histogram(&mut self) {
        if !self.histogram_window.0.is_null() {
            unsafe {
                let _ = SetForegroundWindow(self.histogram_window);
            }
        }
        if self.file_busy || self.load_started.is_some() {
            return;
        }
        if let Some(document) = &self.document {
            self.file_busy = self.file_service.submit(FileTask::Histogram {
                document: document.export_snapshot(),
                region: self.crop_selection,
            });
            if self.file_busy {
                self.histogram_target =
                    (!self.histogram_window.0.is_null()).then_some(self.histogram_window);
                self.update_status(Some(if self.lang() == Language::Spanish {
                    "Calculando histograma…"
                } else {
                    "Calculating histogram…"
                }));
            }
        }
    }

    pub(super) fn open_size_tool(&mut self, tool: SizeTool) {
        if self.file_busy || self.load_started.is_some() {
            return;
        }
        let Some(document) = self.document.as_ref().map(ImageDocument::export_snapshot) else {
            return;
        };
        match tool {
            SizeTool::Canvas => {
                if self.canvas_document == Some(document.id)
                    && unsafe { IsWindow(Some(self.canvas_window)) }.as_bool()
                {
                    if !self.cancel_canvas_pick() {
                        unsafe {
                            let _ = SetForegroundWindow(self.canvas_window);
                        }
                    }
                    return;
                }
                self.close_canvas();
                self.canvas_window = image_tools::show_canvas(
                    self.hwnd,
                    self.lang(),
                    self.settings.is_dark(),
                    document.view_dimensions(),
                );
                self.canvas_document = Some(document.id);
            }
            SizeTool::Resize => {
                self.close_canvas();
                if let Some(operation) = image_tools::show_size(
                    self.hwnd,
                    self.lang(),
                    self.settings.is_dark(),
                    document.view_dimensions(),
                    document.view_resolution(),
                ) {
                    self.submit_image_edit(document, operation);
                }
            }
        }
    }

    fn submit_image_edit(
        &mut self,
        document: ImageDocument,
        operation: crate::imaging::EditOperation,
    ) -> bool {
        self.file_busy = self.file_service.submit(FileTask::EditImage {
            document,
            operation,
        });
        if self.file_busy {
            self.update_status(Some(if self.lang() == Language::Spanish {
                "Aplicando cambios a la imagen…"
            } else {
                "Applying image changes…"
            }));
        }
        self.file_busy
    }

    pub(super) fn image_tool_message(&mut self, window: HWND, action: isize) {
        if window == self.histogram_window {
            if action == image_tools::TOOL_CLOSED {
                self.histogram_window = HWND::default();
            } else if action == image_tools::REFRESH_HISTOGRAM {
                self.open_histogram();
            }
            return;
        }
        if window != self.canvas_window || window.0.is_null() {
            return;
        }
        if action == image_tools::TOOL_CLOSED {
            self.canvas_window = HWND::default();
            self.canvas_picking = false;
            self.canvas_document = None;
            self.canvas_last_revision = None;
            self.update_status(None);
            return;
        }
        if action == image_tools::CANCEL_CANVAS_PICK {
            self.cancel_canvas_pick();
            return;
        }
        if self.file_busy
            || self.load_started.is_some()
            || self
                .document
                .as_ref()
                .is_none_or(|doc| self.canvas_document != Some(doc.id))
        {
            if action == image_tools::PICK_CANVAS_COLOR {
                image_tools::canvas_color(window, None);
            } else {
                image_tools::canvas_result(window, None, false);
            }
            return;
        }
        match action {
            image_tools::PICK_CANVAS_COLOR => {
                self.canvas_picking = true;
                self.update_status(Some(if self.lang() == Language::Spanish {
                    "Haz clic en la imagen para tomar su color. Esc cancela."
                } else {
                    "Click the image to sample its color. Press Esc to cancel."
                }));
                unsafe {
                    let _ = ShowWindow(window, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
                    let _ = SetForegroundWindow(self.hwnd);
                }
            }
            image_tools::APPLY_CANVAS => {
                if let Some(operation) = image_tools::canvas_operation(window)
                    && let Some(document) =
                        self.document.as_ref().map(ImageDocument::export_snapshot)
                    && self.submit_image_edit(document, operation)
                {
                    self.canvas_edit_dialog = window;
                } else {
                    image_tools::canvas_result(window, None, false);
                }
            }
            image_tools::UNDO_CANVAS => {
                let document = self.document.as_mut().expect("canvas document");
                let size =
                    if self.canvas_last_revision == Some(document.revision) && document.undo() {
                        self.canvas_last_revision = Some(document.revision);
                        Some(document.view_dimensions())
                    } else {
                        None
                    };
                if let Some(size) = size {
                    self.refresh_edit_view(size);
                }
                image_tools::canvas_result(window, size, true);
            }
            _ => {}
        }
    }

    pub(super) fn close_canvas(&mut self) {
        let canvas = std::mem::take(&mut self.canvas_window);
        self.canvas_document = None;
        self.canvas_last_revision = None;
        self.canvas_picking = false;
        if !canvas.0.is_null() {
            unsafe {
                let _ = DestroyWindow(canvas);
            }
        }
    }

    pub(super) fn cancel_canvas_pick(&mut self) -> bool {
        if !std::mem::take(&mut self.canvas_picking) {
            return false;
        }
        image_tools::canvas_color(self.canvas_window, None);
        self.present_canvas_after_sampling();
        self.update_status(None);
        true
    }

    pub(super) fn sample_canvas_color(&mut self, point: POINT) -> bool {
        if !self.canvas_picking {
            return false;
        }
        let area = self.image_area();
        if point.x < area.left
            || point.x >= area.right
            || point.y < area.top
            || point.y >= area.bottom
        {
            return false;
        }
        let Some(destination) = self.calculate_view() else {
            return false;
        };
        let Some(document) = &self.document else {
            return false;
        };
        let x = (point.x as f32 - destination.left) / self.zoom;
        let y = (point.y as f32 - destination.top) / self.zoom;
        let (l, t, r, b) = document.rendered_region;
        let frame = document.current_frame();
        if !(l as f32..r as f32).contains(&x) || !(t as f32..b as f32).contains(&y) {
            return false;
        }
        let sx =
            (((x - l as f32) * frame.width as f32 / (r - l) as f32) as u32).min(frame.width - 1);
        let sy =
            (((y - t as f32) * frame.height as f32 / (b - t) as f32) as u32).min(frame.height - 1);
        let start = (sy as usize * frame.width as usize + sx as usize) * 4;
        let [blue, green, red, alpha]: [u8; 4] = frame.pixels[start..start + 4].try_into().unwrap();
        let unpremultiply = |channel| {
            ((u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha).max(1)).min(255)
                as u8
        };
        self.canvas_picking = false;
        image_tools::canvas_color(
            self.canvas_window,
            Some([
                unpremultiply(red),
                unpremultiply(green),
                unpremultiply(blue),
            ]),
        );
        self.present_canvas_after_sampling();
        self.update_status(None);
        true
    }

    fn present_canvas_after_sampling(&self) {
        unsafe {
            let _ = ShowWindow(
                self.canvas_window,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );
            let _ = SetForegroundWindow(self.canvas_window);
        }
    }

    fn refresh_edit_view(&mut self, (width, height): (u32, u32)) {
        self.crop_selection = None;
        self.center = (width as f32 / 2.0, height as f32 / 2.0);
        self.zoom_mode = ZoomMode::Fit;
        self.request_view();
        self.update_status(None);
    }

    pub(super) fn edit<F: FnOnce(&mut ImageDocument)>(&mut self, operation: F) {
        if self.file_busy || self.load_started.is_some() {
            return;
        }
        if let Some(document) = &mut self.document {
            let revision = document.revision;
            operation(document);
            let size = document.view_dimensions();
            if document.revision != revision {
                self.close_canvas();
            }
            self.refresh_edit_view(size);
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
        self.present_startup_window();
        self.bitmaps.clear();
        self.target = None;
        self.chrome.reset_render_resources();
        unsafe {
            if !self.fullscreen {
                let _ = GetWindowRect(self.hwnd, &mut self.saved_rect);
                self.saved_style = WINDOW_STYLE(GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32);
                self.fullscreen = true;
                SetWindowLongPtrW(
                    self.hwnd,
                    GWL_STYLE,
                    (WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN).0 as isize,
                );
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
        self.layout_video();
        self.reveal_video_panel();
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
