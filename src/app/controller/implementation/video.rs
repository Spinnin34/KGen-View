use super::*;
use crate::{
    platform::playback::{Control, Phase, Playback, PlaybackStatus, report_failure},
    ui::video::{Bar, Click, message as video_message},
};
use windows::Win32::UI::WindowsAndMessaging::{
    SW_HIDE, WS_CHILD, WS_DISABLED, WS_EX_NOPARENTNOTIFY,
};

const SS_BLACKRECT: WINDOW_STYLE = WINDOW_STYLE(4);

pub(in crate::app::controller) struct VideoView {
    surface: HWND,
    playback: Option<Playback>,
    active: bool,
    pub(super) closing: bool,
    status: PlaybackStatus,
    started: Option<Instant>,
    volume: i32,
    muted: bool,
}

impl VideoView {
    fn new(hwnd: HWND) -> Result<Self, WinError> {
        let surface = unsafe {
            CreateWindowExW(
                WS_EX_NOPARENTNOTIFY,
                w!("STATIC"),
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN | WS_DISABLED | SS_BLACKRECT,
                0,
                0,
                0,
                0,
                Some(hwnd),
                None,
                Some(HINSTANCE(GetModuleHandleW(None)?.0)),
                None,
            )?
        };
        let playback = Playback::start(hwnd, surface)
            .inspect_err(|error| report_failure("start worker", error.raw_os_error().unwrap_or(0)))
            .ok();
        Ok(Self {
            surface,
            playback,
            active: false,
            closing: false,
            status: PlaybackStatus::new(Phase::Opening),
            started: None,
            volume: 100,
            muted: false,
        })
    }
}

impl AppState {
    pub(super) fn video(&self) -> Option<&VideoView> {
        self.video.as_deref().filter(|view| view.active)
    }

    pub(super) fn video_in_progress(&self) -> bool {
        self.video().is_some_and(|view| {
            matches!(
                view.status.phase,
                Phase::Opening | Phase::Playing | Phase::Paused
            )
        })
    }

    pub(super) fn start_video(&mut self, path: PathBuf) {
        let same_folder =
            self.catalog.files.first().and_then(|known| known.parent()) == path.parent();
        if same_folder {
            self.catalog.update_index(&path);
        } else {
            self.catalog = FolderCatalog::default();
            self.catalog_stamp = None;
        }
        self.archive_fallback = None;
        self.retain_active_archive_session();
        self.active_path = Some(path.clone());
        self.document = None;
        self.bitmaps.clear();
        self.first_visible = None;
        self.selection_start = None;
        self.selection_end = None;
        self.crop_selection = None;
        self.set_animation_timer();
        self.refresh_restart_registration();
        self.folder_watcher
            .watch(path.parent().map(Path::to_path_buf));
        self.folder_dirty = false;
        self.update_title();

        let started = self.load_started.take();
        if self.video.is_none() {
            self.video = VideoView::new(self.hwnd)
                .inspect_err(|error| report_failure("create surface", error.code().0))
                .ok()
                .map(Box::new);
        }
        let Some(view) = self.video.as_deref_mut() else {
            self.update_status(Some(video_message(self.lang(), Phase::Failed)));
            return;
        };
        view.active = true;
        view.started = started;
        view.status = PlaybackStatus::new(if view.playback.is_some() {
            Phase::Opening
        } else {
            Phase::Failed
        });
        self.layout_video();
        let view = self.video.as_deref().unwrap();
        if let Some(playback) = &view.playback {
            playback.open(
                self.generation,
                display_path(&path),
                view.volume,
                view.muted,
            );
        }
        self.update_status(None);
    }

    pub(super) fn stop_video(&mut self) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.active = false;
            self.active_path = None;
            unsafe {
                let _ = ShowWindow(view.surface, SW_HIDE);
            }
            if let Some(playback) = &view.playback {
                playback.stop(self.generation);
            }
        }
    }

    pub(super) fn close_video(&mut self) -> bool {
        let Some(view) = self.video.as_deref_mut() else {
            return false;
        };
        let Some(playback) = &view.playback else {
            return false;
        };
        if !view.closing {
            view.closing = true;
            playback.close();
        }
        true
    }

    pub(super) fn layout_video(&self) {
        let Some(view) = self.video() else { return };
        let chrome = self.chrome_layout();
        let bar = Bar::new(&chrome);
        let area = chrome.image;
        let height = bar.bounds.top.round() as i32 - area.top.round() as i32;
        unsafe {
            if height <= 0 || matches!(view.status.phase, Phase::MissingEngine | Phase::Failed) {
                let _ = ShowWindow(view.surface, SW_HIDE);
            } else {
                let _ = SetWindowPos(
                    view.surface,
                    None,
                    0,
                    area.top.round() as i32,
                    area.right.round() as i32,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }

    pub(super) fn accept_video_status(&mut self) {
        let Some(view) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && !view.closing)
        else {
            return;
        };
        let Some((generation, status)) = view.playback.as_ref().and_then(Playback::take) else {
            return;
        };
        if generation != self.generation {
            return;
        }
        if status.video_ready
            && let Some(started) = view.started.take()
        {
            crate::diagnostics::elapsed("video_output_ready", started.elapsed());
        }
        let phase_changed = status.phase != view.status.phase;
        view.status = status;
        if phase_changed && matches!(status.phase, Phase::MissingEngine | Phase::Failed) {
            self.layout_video();
        }
        self.invalidate_video_bar();
    }

    fn invalidate_video_bar(&self) {
        let bounds = Bar::new(&self.chrome_layout()).bounds;
        let area = RECT {
            left: bounds.left as i32,
            top: bounds.top.round() as i32,
            right: bounds.right.round() as i32,
            bottom: bounds.bottom.round() as i32,
        };
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), Some(&area), false);
        }
    }

    pub(super) unsafe fn draw_video_bar(
        &self,
        hdc: windows::Win32::Graphics::Gdi::HDC,
        layout: &ChromeLayout,
    ) {
        if let Some(view) = self.video() {
            unsafe {
                crate::ui::video::draw(
                    hdc,
                    &Bar::new(layout),
                    view.status,
                    view.volume,
                    view.muted,
                    self.lang(),
                    self.settings.is_dark(),
                )
            };
        }
    }

    pub(super) fn paint_video_update(
        &self,
        hdc: windows::Win32::Graphics::Gdi::HDC,
        dirty: RECT,
    ) -> bool {
        if self.video().is_none() {
            return false;
        }
        let layout = self.chrome_layout();
        let bounds = Bar::new(&layout).bounds;
        if dirty.top < bounds.top.round() as i32 || dirty.bottom > bounds.bottom.round() as i32 {
            return false;
        }
        // Clock updates repaint the controls without presenting the whole
        // Direct2D surface underneath LibVLC's child window.
        unsafe { self.draw_video_bar(hdc, &layout) };
        true
    }

    pub(super) fn click_video(&mut self, point: POINT, layout: &ChromeLayout) -> bool {
        let Some(view) = self.video() else {
            return false;
        };
        let bar = Bar::new(layout);
        if !video_message(self.lang(), view.status.phase).is_empty() {
            return layout.image.contains(point);
        }
        match bar.hit(point) {
            Some(Click::Pause) => self.video_pause(),
            Some(Click::Mute) => self.video_audio(0, true),
            Some(Click::Volume(volume)) => self.set_video_volume(volume, false),
            Some(Click::Seek(fraction)) if view.status.seekable => {
                self.video_seek((view.status.duration_ms as f64 * fraction) as i64);
            }
            _ if layout.image.contains(point) && !bar.bounds.contains(point) => {
                self.video_pause();
            }
            _ => {}
        }
        layout.image.contains(point)
    }

    pub(super) fn video_pause(&self) {
        if let Some(player) = self.video().and_then(|view| view.playback.as_ref()) {
            player.control(self.generation, Control::TogglePause);
        }
    }

    pub(super) fn video_seek_relative(&self, delta_ms: i64) {
        if let Some(view) = self.video().filter(|view| view.status.seekable) {
            self.video_seek(view.status.elapsed_ms.saturating_add(delta_ms));
        }
    }

    fn video_seek(&self, position_ms: i64) {
        if let Some(player) = self.video().and_then(|view| view.playback.as_ref()) {
            player.control(self.generation, Control::Seek(position_ms));
        }
    }

    pub(super) fn video_audio(&mut self, delta: i32, toggle_mute: bool) {
        if let Some(view) = self.video() {
            let volume = view.volume.saturating_add(delta);
            let muted = toggle_mute && !view.muted;
            self.set_video_volume(volume, muted);
        }
    }

    fn set_video_volume(&mut self, volume: i32, muted: bool) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.volume = volume.clamp(0, 100);
            view.muted = muted;
            if let Some(player) = &view.playback {
                player.control(
                    self.generation,
                    Control::Audio {
                        volume: view.volume,
                        muted,
                    },
                );
            }
            self.invalidate_video_bar();
        }
    }
}
