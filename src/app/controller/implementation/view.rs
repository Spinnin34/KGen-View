use super::*;

impl AppState {
    pub(super) fn schedule_view(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_VIEW);
            if !IsIconic(self.hwnd).as_bool() {
                let _ = SetTimer(Some(self.hwnd), TIMER_VIEW, 80, None);
            }
        }
    }

    pub(super) fn request_view(&mut self) {
        if self.load_started.is_some() || unsafe { IsIconic(self.hwnd).as_bool() } {
            return;
        }
        let Some(destination) = self.calculate_view() else {
            return;
        };
        let area = self.image_area();
        let document = self.document.as_ref().expect("document view");
        let (w, h) = document.view_dimensions();
        let l = (((area.left as f32 - destination.left) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(w - 1);
        let t = (((area.top as f32 - destination.top) / self.zoom)
            .floor()
            .max(0.0) as u32)
            .min(h - 1);
        let r = (((area.right as f32 - destination.left) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .clamp(l + 1, w);
        let b = (((area.bottom as f32 - destination.top) / self.zoom)
            .ceil()
            .max(0.0) as u32)
            .clamp(t + 1, h);
        let target = (
            ((r - l) as f32 * self.zoom).ceil().max(1.0) as u32,
            ((b - t) as f32 * self.zoom).ceil().max(1.0) as u32,
        );
        let key = (
            self.generation,
            document.revision,
            self.requested_page,
            (l, t, r, b),
            target,
        );
        if self.view_key == Some(key) {
            return;
        }
        self.view_key = Some(key);
        let mut request = DecodeRequest::preview(target);
        request.page = self.requested_page;
        request.region = Some((l, t, r, b));
        self.view_request = self.view_request.wrapping_add(1);
        self.view_pending = true;
        self.loader.submit_view(
            self.generation,
            self.view_request,
            document.clone(),
            request,
            self.hwnd,
        );
    }

    pub(super) fn change_frame(&mut self, offset: isize) {
        let Some(document) = &self.document else {
            return;
        };
        if document.frame_count() <= 1 || self.load_started.is_some() {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ANIMATION);
        }
        self.requested_page = (self.requested_page as isize + offset)
            .rem_euclid(document.frame_count() as isize) as usize;
        self.request_view();
    }

    pub(super) fn accept_view(&mut self, payload: ViewPayload) {
        if payload.generation != self.generation || payload.request_id != self.view_request {
            return;
        }
        self.view_pending = false;
        if payload.result.as_ref().is_ok_and(|rendered| {
            self.document.as_ref().is_none_or(|current| {
                current.id != rendered.id || current.revision != rendered.revision
            })
        }) {
            self.view_key = None;
            self.schedule_view();
            return;
        }
        match payload.result {
            Ok(document) => {
                let size_changed = self
                    .document
                    .as_ref()
                    .is_some_and(|current| current.original_size != document.original_size);
                self.document = Some(document);
                if size_changed {
                    self.view_key = None;
                    self.schedule_view();
                }
                self.invalidate_image();
                self.set_animation_timer();
                if self.prefetch_enabled {
                    self.start_prefetch();
                }
            }
            Err(error) => {
                self.view_key = None;
                self.requested_page = self
                    .document
                    .as_ref()
                    .map(|doc| doc.frame_index)
                    .unwrap_or(0);
                self.animation_playing = false;
                self.update_status(Some(&error.to_string()));
            }
        }
    }
}
