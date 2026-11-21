use super::*;
use crate::{
    platform::playback::{
        Control, MAX_VOLUME, MediaDetails, Phase, Playback, PlaybackStatus, Preferences,
        report_failure,
    },
    ui::video::{
        Action, EdgeLayout, EdgeZone, Hit, Layer, LayerKind, MenuCommand, MenuHit, MenuPage,
        MenuRow, Model, Painter, PanelLayout, ProgressLine, RATES, VideoMenu, VolumeHit,
        VolumeLayout, VolumePainter, rate_label, time_label,
    },
};
use windows::Win32::UI::{
    HiDpi::GetDpiForWindow,
    WindowsAndMessaging::{
        GA_ROOTOWNER, GetAncestor, GetCursorPos, IsWindowVisible, SW_HIDE, SetCursor,
        WM_CAPTURECHANGED, WM_SETCURSOR, WS_CHILD, WS_DISABLED, WS_EX_NOPARENTNOTIFY,
        WindowFromPoint,
    },
};

mod clock;
mod menu;
use clock::Playhead;

const VIDEO_SURFACE_CLASS: PCWSTR = windows::core::w!("KGenView.VideoSurface");
pub(super) const TIMER_VIDEO_PANEL: usize = 9;
const ACTIVE_INTERVAL_MS: u32 = 15;
const PROGRESS_INTERVAL_MS: u32 = 33;
const IDLE_INTERVAL_MS: u32 = 70;
const HIDE_AFTER: std::time::Duration = std::time::Duration::from_millis(2400);

fn register_video_surface_class() -> Result<(), WinError> {
    use windows::Win32::{
        Foundation::ERROR_CLASS_ALREADY_EXISTS,
        Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH},
        UI::WindowsAndMessaging::{CS_HREDRAW, CS_VREDRAW, IDC_ARROW, LoadCursorW},
    };

    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(video_surface_window_proc),
        hInstance: unsafe { HINSTANCE(GetModuleHandleW(None)?.0) },
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: unsafe { HBRUSH(GetStockObject(BLACK_BRUSH).0) },
        lpszClassName: VIDEO_SURFACE_CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        let error = WinError::from_thread();
        if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    Ok(())
}

unsafe extern "system" fn video_surface_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_SETCURSOR && crate::ui::video::video_cursor_hidden() {
        unsafe { SetCursor(None) };
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub(in crate::app::controller) struct VideoView {
    surface: HWND,
    playback: Option<Playback>,
    active: bool,
    pub(super) closing: bool,
    status: PlaybackStatus,
    media: Option<Arc<MediaDetails>>,
    previous_position: Option<i64>,
    playhead: Playhead,
    started: Option<Instant>,
    resize_pending: bool,
    preferences: Preferences,
    panel: Option<VideoPanel>,
    notice: Option<String>,
}

struct VideoPanel {
    layer: Layer,
    painter: Option<Painter>,
    layout: Option<PanelLayout>,
    edges: Option<EdgeLayout>,
    progress: Option<ProgressLine>,
    volume: Option<SidePanel>,
    cursor: Option<POINT>,
    zone: EdgeZone,
    reveal: Reveal,
    hover: Option<Hit>,
    seek_hover: Option<f32>,
    focus: Option<Action>,
    pressed: Option<Hit>,
    drag: Option<f32>,
    menu: Option<VideoMenu>,
    timer_interval: u32,
    timeline_key: Option<(i32, i64)>,
    cursor_activity: Instant,
}

impl VideoPanel {
    fn new(layer: Layer, progress: Option<Layer>, volume: Option<Layer>) -> Self {
        Self {
            layer,
            painter: None,
            layout: None,
            edges: None,
            progress: progress.map(ProgressLine::new),
            volume: volume.map(SidePanel::new),
            cursor: None,
            zone: EdgeZone::None,
            reveal: Reveal::default(),
            hover: None,
            seek_hover: None,
            focus: None,
            pressed: None,
            drag: None,
            menu: None,
            timer_interval: 0,
            timeline_key: None,
            cursor_activity: Instant::now(),
        }
    }

    fn set_timer(&mut self, hwnd: HWND, interval: u32) {
        if self.timer_interval != interval
            && unsafe { SetTimer(Some(hwnd), TIMER_VIDEO_PANEL, interval, None) } != 0
        {
            self.timer_interval = interval;
        }
    }

    fn timeline_changed(&self, status: PlaybackStatus) -> bool {
        if self.layer.visible {
            self.layout.as_ref().is_some_and(|layout| {
                self.timeline_key != Some(layout.timeline_key(status, self.drag))
            })
        } else {
            self.progress
                .as_ref()
                .is_some_and(|progress| progress.position_changed(status))
        }
    }

    fn clear_input(&mut self) {
        self.hover = None;
        self.seek_hover = None;
        self.focus = None;
        self.pressed = None;
        self.drag = None;
        self.zone = EdgeZone::None;
        self.cursor = None;
        self.cursor_activity = Instant::now();
        self.reveal = Reveal::default();
        if let Some(menu) = &mut self.menu {
            menu.close();
        }
        if let Some(volume) = &mut self.volume {
            volume.clear_input();
        }
    }

    fn wants_visible(&self, pinned: bool, now: Instant) -> bool {
        pinned
            || self.reveal.visible(now)
            || self.focus.is_some()
            || self.pressed.is_some()
            || self.menu.as_ref().is_some_and(VideoMenu::opened)
    }
}

#[derive(Default)]
struct Reveal {
    pointer_until: Option<Instant>,
    explicit_until: Option<Instant>,
}

impl Reveal {
    fn pointer(&mut self, in_zone: bool, motion_or_entry: bool, now: Instant) {
        if !in_zone {
            self.pointer_until = None;
        } else if motion_or_entry {
            self.pointer_until = Some(now + HIDE_AFTER);
        }
    }

    fn show(&mut self) {
        self.explicit_until = Some(Instant::now() + HIDE_AFTER);
    }

    fn visible(&self, now: Instant) -> bool {
        self.pointer_until.is_some_and(|until| now < until)
            || self.explicit_until.is_some_and(|until| now < until)
    }
}

struct SidePanel {
    layer: Layer,
    painter: Option<VolumePainter>,
    layout: Option<VolumeLayout>,
    last_frame: Option<VolumeFrame>,
    reveal: Reveal,
    hover: Option<VolumeHit>,
    pressed: Option<VolumeHit>,
}

#[derive(PartialEq)]
struct VolumeFrame {
    origin: POINT,
    size: windows::Win32::Foundation::SIZE,
    value: i32,
    muted: bool,
    hover: Option<VolumeHit>,
    pressed: Option<VolumeHit>,
}

impl SidePanel {
    fn new(layer: Layer) -> Self {
        Self {
            layer,
            painter: None,
            layout: None,
            last_frame: None,
            reveal: Reveal::default(),
            hover: None,
            pressed: None,
        }
    }

    fn clear_input(&mut self) {
        self.reveal = Reveal::default();
        self.hover = None;
        self.pressed = None;
    }

    fn wants_visible(&self, now: Instant) -> bool {
        self.layout.is_some() && (self.reveal.visible(now) || self.pressed.is_some())
    }

    fn draw(&mut self, owner: HWND, preferences: Preferences) -> Result<(), WinError> {
        let Some(layout) = self.layout else {
            return Ok(());
        };
        let mut origin = layout.offset;
        unsafe {
            let _ = ClientToScreen(owner, &mut origin);
        }
        let frame = VolumeFrame {
            origin,
            size: layout.size,
            value: preferences.volume,
            muted: preferences.muted,
            hover: self.hover,
            pressed: self.pressed,
        };
        if self.layer.visible && self.last_frame.as_ref() == Some(&frame) {
            return Ok(());
        }
        self.layer.render(origin, layout.size, |target| {
            VolumePainter::draw(
                &mut self.painter,
                target,
                layout,
                preferences,
                self.hover,
                self.pressed == Some(VolumeHit::Track),
            )
        })?;
        self.last_frame = Some(frame);
        Ok(())
    }
}

impl VideoView {
    fn new(hwnd: HWND, factory: &ID2D1Factory) -> Result<Self, WinError> {
        register_video_surface_class()?;
        let surface = unsafe {
            CreateWindowExW(
                WS_EX_NOPARENTNOTIFY,
                VIDEO_SURFACE_CLASS,
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN | WS_DISABLED,
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
        let layer = |kind| {
            Layer::new(hwnd, factory, kind)
                .inspect_err(|error| report_failure("create video overlay", error.code().0))
                .ok()
        };
        let panel = layer(LayerKind::Controls).map(|controls| {
            VideoPanel::new(
                controls,
                layer(LayerKind::Progress),
                layer(LayerKind::Volume),
            )
        });
        Ok(Self {
            surface,
            playback,
            panel,
            active: false,
            closing: false,
            status: PlaybackStatus::new(Phase::Opening),
            media: None,
            previous_position: None,
            playhead: Playhead::new(Instant::now()),
            started: None,
            resize_pending: false,
            preferences: Preferences::default(),
            notice: None,
        })
    }

    fn display_status(&self, now: Instant) -> PlaybackStatus {
        PlaybackStatus {
            elapsed_ms: self
                .playhead
                .position(self.status, self.preferences.rate, now),
            ..self.status
        }
    }

    fn failed(&self) -> bool {
        matches!(self.status.phase, Phase::Failed | Phase::MissingEngine)
    }

    fn controls_pinned(&self) -> bool {
        self.failed()
            || self.notice.is_some()
            || (self.status.phase == Phase::Opening && self.status.dimensions.is_none())
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
            ) || (view.status.phase == Phase::Ended && view.preferences.looping)
        })
    }

    pub(super) fn start_video(&mut self, path: PathBuf) {
        if self.catalog.matches(&path, MediaKind::Video) {
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
        self.target = None;
        self.chrome.reset_render_resources();
        self.chrome.toolbar = Default::default();
        self.chrome.menu.hovered = None;
        self.chrome.menu.pressed = None;
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
        let saved_preferences = Preferences {
            volume: self.settings.video_volume,
            muted: self.settings.video_muted,
            looping: self.settings.video_looping,
            rate: match self.settings.video_rate_percent {
                50 => 0.5,
                75 => 0.75,
                125 => 1.25,
                150 => 1.5,
                200 => 2.0,
                _ => 1.0,
            },
            subtitle_track: self.settings.video_subtitle_track,
        };
        if self.video.is_none() {
            self.video = VideoView::new(self.hwnd, &self.d2d_factory)
                .inspect_err(|error| report_failure("create surface", error.code().0))
                .ok()
                .map(Box::new);
        }
        let Some(view) = self.video.as_deref_mut() else {
            self.update_status(Some(if self.lang() == Language::Spanish {
                "No se pudo crear el reproductor."
            } else {
                "Could not create the player."
            }));
            self.present_startup_window();
            return;
        };
        view.active = true;
        view.preferences = saved_preferences;
        view.media = None;
        view.previous_position = None;
        view.started = started;
        view.resize_pending = true;
        view.notice = None;
        view.status = PlaybackStatus::new(if view.playback.is_some() {
            Phase::Opening
        } else {
            Phase::Failed
        });
        view.playhead = Playhead::new(Instant::now());
        if let Some(panel) = &mut view.panel {
            panel.clear_input();
            panel.set_timer(self.hwnd, IDLE_INTERVAL_MS);
        }
        self.layout_video();
        let view = self.video.as_deref().unwrap();
        if let Some(playback) = &view.playback {
            playback.open(self.generation, display_path(&path), view.preferences);
        } else {
            self.present_startup_window();
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub(super) fn stop_video(&mut self) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.active = false;
            view.media = None;
            self.active_path = None;
            if let Some(panel) = &mut view.panel {
                panel.layer.reset();
                panel.painter = None;
                panel.layout = None;
                panel.edges = None;
                panel.timeline_key = None;
                panel.timer_interval = 0;
                if let Some(progress) = &mut panel.progress {
                    progress.reset();
                }
                if let Some(volume) = &mut panel.volume {
                    volume.layer.reset();
                    volume.painter = None;
                    volume.layout = None;
                    volume.last_frame = None;
                }
                panel.clear_input();
                panel.menu = None;
            }
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_VIDEO_PANEL);
                let _ = ShowWindow(view.surface, SW_HIDE);
            }
            crate::ui::video::set_video_cursor_hidden(false);
            if let Some(playback) = &view.playback {
                playback.stop(self.generation);
            }
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
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
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_VIDEO_PANEL);
            }
            if let Some(panel) = &mut view.panel {
                panel.layer.hide();
                if let Some(menu) = &mut panel.menu {
                    menu.close();
                }
                if let Some(progress) = &mut panel.progress {
                    progress.hide();
                }
                if let Some(volume) = &mut panel.volume {
                    volume.layer.hide();
                }
            }
            crate::ui::video::set_video_cursor_hidden(false);
            playback.close();
        }
        true
    }

    pub(super) fn layout_video(&mut self) {
        if self.settings.auto_resize
            && !self.keeps_window_size()
            && !unsafe { IsIconic(self.hwnd).as_bool() }
            && let Some(dimensions) = self.take_video_resize()
        {
            self.fit_open_media(dimensions, true);
        }
        let Some(view) = self.video() else { return };
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut client);
            if view.failed() || IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(view.surface, SW_HIDE);
            } else {
                let _ = SetWindowPos(
                    view.surface,
                    None,
                    0,
                    0,
                    client.right,
                    client.bottom,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        self.layout_video_panel();
    }

    pub(super) fn layout_video_panel(&mut self) {
        let Some(panel) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active)
            .and_then(|view| view.panel.as_mut())
        else {
            return;
        };
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut client);
        }
        let scale = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0;
        panel.layout = PanelLayout::new(client, scale, self.update_available.is_some());
        let edges = EdgeLayout::new(client, scale, panel.layout.as_ref());
        panel.edges = Some(edges);
        if let Some(volume) = &mut panel.volume {
            volume.layout = edges.volume();
        }
        self.render_video_panel();
    }

    pub(super) fn accept_video_status(&mut self) {
        let Some(view) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && !view.closing)
        else {
            return;
        };
        let Some((generation, status, media)) = view.playback.as_ref().and_then(Playback::take)
        else {
            return;
        };
        if generation != self.generation {
            return;
        }
        let media_changed = match (&view.media, &media) {
            (Some(previous), Some(current)) => !Arc::ptr_eq(previous, current),
            (None, None) => false,
            _ => true,
        };
        view.media = media;
        if status.video_ready
            && let Some(started) = view.started.take()
        {
            crate::diagnostics::elapsed("video_output_ready", started.elapsed());
        }
        let new_size = status.dimensions.is_some() && view.status.dimensions.is_none();
        let was_failed = view.failed();
        let now = Instant::now();
        let state_changed = view.status
            != PlaybackStatus {
                elapsed_ms: view.status.elapsed_ms,
                ..status
            };
        view.playhead
            .sample(view.status, status, view.preferences.rate, now);
        let menu_state_changed = view.status.phase != status.phase
            || view.status.audio_track != status.audio_track
            || view.status.subtitle_track != status.subtitle_track;
        view.status = status;
        if new_size || was_failed != view.failed() {
            self.layout_video();
        } else if media_changed
            || state_changed
            || view
                .panel
                .as_ref()
                .is_some_and(|panel| panel.timeline_changed(view.display_status(now)))
        {
            self.render_video_panel();
        }
        if menu_state_changed || media_changed {
            self.refresh_video_menu();
            if self.video_menu_opened() {
                self.render_video_panel();
            }
        }
        if new_size
            || matches!(
                status.phase,
                Phase::Ended | Phase::MissingEngine | Phase::Failed
            )
        {
            self.present_startup_window();
        }
    }

    fn take_video_resize(&mut self) -> Option<(u32, u32)> {
        let view = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && view.resize_pending)?;
        let size = view.status.dimensions?;
        view.resize_pending = false;
        Some(size)
    }

    fn video_model<'a>(
        view: &'a VideoView,
        panel: &VideoPanel,
        language: Language,
        fullscreen: bool,
        can_navigate: bool,
        update_busy: bool,
        now: Instant,
    ) -> Model<'a> {
        Model {
            status: view.display_status(now),
            preferences: view.preferences,
            language,
            fullscreen,
            volume_open: panel
                .volume
                .as_ref()
                .is_some_and(|volume| volume.wants_visible(now)),
            can_navigate,
            update_busy,
            hover: panel.hover,
            seek_hover: panel.seek_hover,
            focus: panel.focus,
            drag: panel.drag,
            notice: view.notice.as_deref(),
            media: view.media.as_deref(),
        }
    }

    pub(super) fn render_video_panel(&mut self) {
        let language = self.lang();
        let Some(view) = self.video.as_deref_mut().filter(|view| view.active) else {
            return;
        };
        let Some(mut panel) = view.panel.take() else {
            return;
        };
        let allowed = !view.closing
            && unsafe { IsWindowVisible(self.hwnd).as_bool() && !IsIconic(self.hwnd).as_bool() };
        let foreground_window = unsafe { GetForegroundWindow() };
        let foreground = allowed
            && (foreground_window == self.hwnd
                || unsafe { GetAncestor(foreground_window, GA_ROOTOWNER) == self.hwnd });
        let now = Instant::now();
        let status = view.display_status(now);
        let show = foreground && panel.wants_visible(view.controls_pinned(), now);
        if show && let Some(layout) = &panel.layout {
            let model = Self::video_model(
                view,
                &panel,
                language,
                self.fullscreen,
                self.catalog.files.len() > 1,
                self.update_download_busy,
                now,
            );
            let mut origin = layout.offset;
            unsafe {
                let _ = ClientToScreen(self.hwnd, &mut origin);
            }
            let result = panel.layer.render(origin, layout.size, |target| {
                Painter::draw(&mut panel.painter, target, layout, &model)
            });
            if let Err(error) = result {
                report_failure("draw floating controls", error.code().0);
                panel.layer.hide();
            } else {
                panel.timeline_key = Some(layout.timeline_key(model.status, model.drag));
            }
        } else {
            panel.layer.hide();
        }
        let menu_open = panel.menu.as_ref().is_some_and(VideoMenu::opened);
        if let Some(volume) = &mut panel.volume {
            if foreground && !menu_open && volume.wants_visible(now) {
                if let Err(error) = volume.draw(self.hwnd, view.preferences) {
                    report_failure("draw volume controls", error.code().0);
                    volume.layer.hide();
                }
            } else {
                volume.layer.hide();
            }
        }
        if let Some(progress) = &mut panel.progress {
            let show = foreground && !panel.layer.visible && !view.failed();
            if show && let Some((mut origin, size)) = panel.edges.and_then(EdgeLayout::progress) {
                unsafe {
                    let _ = ClientToScreen(self.hwnd, &mut origin);
                }
                if let Err(error) = progress.draw(origin, size, status) {
                    report_failure("draw collapsed progress", error.code().0);
                    progress.hide();
                }
            } else {
                progress.hide();
            }
        }
        if let Some(menu) = &mut panel.menu {
            if foreground
                && menu_open
                && let Some(dock) = &panel.layout
            {
                if let Err(error) = menu.draw(self.hwnd, dock) {
                    report_failure("draw video settings", error.code().0);
                    menu.close();
                }
            } else {
                menu.close();
            }
        }
        let interactive = panel.layer.visible
            || panel
                .volume
                .as_ref()
                .is_some_and(|volume| volume.layer.visible);
        let interval = if interactive {
            ACTIVE_INTERVAL_MS
        } else if status.phase == Phase::Playing
            && panel
                .progress
                .as_ref()
                .is_some_and(ProgressLine::is_visible)
        {
            PROGRESS_INTERVAL_MS
        } else {
            IDLE_INTERVAL_MS
        };
        if !view.closing {
            panel.set_timer(self.hwnd, interval);
        }
        view.panel = Some(panel);
    }

    pub(super) fn video_tick(&mut self) {
        let Some(view) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && !view.closing)
        else {
            return;
        };
        let pinned = view.controls_pinned();
        let now = Instant::now();
        let status = view.display_status(now);
        let chapters = view
            .media
            .as_ref()
            .map_or(&[][..], |media| media.chapters.as_slice());
        let Some(panel) = &mut view.panel else {
            return;
        };
        let foreground_window = unsafe { GetForegroundWindow() };
        let foreground = foreground_window == self.hwnd
            || unsafe { GetAncestor(foreground_window, GA_ROOTOWNER) == self.hwnd };
        if !foreground {
            crate::ui::video::set_video_cursor_hidden(false);
            let visible = panel.layer.visible
                || panel.menu.as_ref().is_some_and(VideoMenu::opened)
                || panel
                    .volume
                    .as_ref()
                    .is_some_and(|volume| volume.layer.visible);
            panel.clear_input();
            if visible || panel.timeline_changed(status) {
                self.render_video_panel();
            }
            return;
        }
        let mut screen = POINT::default();
        if unsafe { GetCursorPos(&mut screen) }.is_err() {
            return;
        }
        let mut cursor = screen;
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut cursor);
        }
        let side_layout = panel.volume.as_ref().and_then(|volume| volume.layout);
        let bottom_local = panel.layout.as_ref().map(|layout| POINT {
            x: cursor.x - layout.offset.x,
            y: cursor.y - layout.offset.y,
        });
        let edge = panel
            .edges
            .map_or(EdgeZone::None, |edges| edges.zone(cursor));
        let mut zone = edge;
        if panel.layer.visible
            && panel
                .layout
                .as_ref()
                .zip(bottom_local)
                .is_some_and(|(layout, local)| layout.contains(local))
        {
            zone = EdgeZone::Bottom;
        }
        if panel
            .volume
            .as_ref()
            .is_some_and(|volume| volume.layer.visible)
            && side_layout.is_some_and(|layout| layout.contains(layout.local(cursor)))
        {
            zone = EdgeZone::Side;
        }
        let moved_screen = panel.cursor != Some(screen);
        if moved_screen {
            panel.cursor_activity = now;
        }
        if zone != EdgeZone::None
            && (moved_screen || edge != panel.zone)
            && unsafe { GetAncestor(WindowFromPoint(screen), GA_ROOTOWNER) != self.hwnd }
        {
            zone = EdgeZone::None;
        }
        let moved = moved_screen || panel.zone != edge;
        panel.cursor = Some(screen);
        panel.zone = edge;
        let menu_open = panel.menu.as_ref().is_some_and(VideoMenu::opened);
        if menu_open
            && panel
                .menu
                .as_ref()
                .is_some_and(|menu| menu.local(cursor).is_some())
        {
            zone = EdgeZone::None;
        }
        panel.reveal.pointer(zone == EdgeZone::Bottom, moved, now);
        let before = (panel.hover, panel.seek_hover, panel.drag, panel.focus);
        if moved && zone == EdgeZone::Bottom {
            panel.focus = None;
        }
        if let Some((layout, local)) = panel.layout.as_ref().zip(bottom_local) {
            panel.hover = (zone == EdgeZone::Bottom && panel.layer.visible)
                .then(|| layout.hit(local))
                .flatten();
            panel.seek_hover = (status.seekable && panel.hover == Some(Hit::Seek))
                .then(|| layout.seek_fraction(local.x, status.duration_ms, chapters));
            if panel.drag.is_some() {
                panel.drag = Some(layout.seek_fraction(local.x, status.duration_ms, chapters));
            }
        }
        let mut redraw = (panel.wants_visible(pinned, now) && panel.layout.is_some())
            != panel.layer.visible
            || before != (panel.hover, panel.seek_hover, panel.drag, panel.focus)
            || panel.timeline_changed(status);
        if let Some(menu) = &mut panel.menu
            && menu.opened()
        {
            redraw |= menu.hover_at(cursor, moved_screen);
        }
        let mut set_volume = None;
        if !menu_open
            && let Some(volume) = &mut panel.volume
            && let Some(layout) = side_layout
        {
            volume.reveal.pointer(zone == EdgeZone::Side, moved, now);
            let local = layout.local(cursor);
            let hover = (zone == EdgeZone::Side && volume.layer.visible)
                .then(|| layout.hit(local))
                .flatten();
            redraw |= hover != volume.hover || volume.wants_visible(now) != volume.layer.visible;
            volume.hover = hover;
            if volume.pressed == Some(VolumeHit::Track) {
                let value = layout.value(local.y);
                if view.preferences.volume != value || view.preferences.muted {
                    set_volume = Some(value);
                    redraw = true;
                }
            }
        }
        let inside_player =
            unsafe { GetAncestor(WindowFromPoint(screen), GA_ROOTOWNER) == self.hwnd };
        let controls_visible = panel.wants_visible(pinned, now);
        let menu_open = panel.menu.as_ref().is_some_and(VideoMenu::opened);
        let volume_visible = panel
            .volume
            .as_ref()
            .is_some_and(|volume| volume.wants_visible(now));
        let hide_cursor = inside_player
            && matches!(status.phase, Phase::Playing | Phase::Paused | Phase::Ended)
            && !controls_visible
            && !menu_open
            && !volume_visible
            && now.duration_since(panel.cursor_activity) >= HIDE_AFTER;
        crate::ui::video::set_video_cursor_hidden(hide_cursor);
        if let Some(value) = set_volume {
            self.set_video_volume(value, false);
        }
        if redraw {
            self.render_video_panel();
        }
    }

    pub(super) fn video_activation_changed(&mut self, active: bool) {
        crate::ui::video::set_video_cursor_hidden(false);
        if let Some(panel) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && !view.closing)
            .and_then(|view| view.panel.as_mut())
        {
            panel.cursor = None;
            panel.cursor_activity = Instant::now();
        }
        if active {
            self.video_tick();
            self.render_video_panel();
        } else {
            self.hide_video_panel();
        }
    }

    pub(super) fn video_notice(&mut self, notice: Option<&str>) -> bool {
        let Some(view) = self.video.as_deref_mut().filter(|view| view.active) else {
            return false;
        };
        view.notice = notice.map(str::to_owned);
        self.render_video_panel();
        true
    }

    pub(super) fn reveal_video_panel(&mut self) {
        if let Some(panel) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active)
            .and_then(|view| view.panel.as_mut())
        {
            panel.reveal.show();
            self.render_video_panel();
        }
    }

    pub(super) fn video_canvas_click(&mut self) -> bool {
        if self.video_menu_opened() {
            self.close_video_menu();
            self.render_video_panel();
            return true;
        }
        if self.video().is_none() {
            return false;
        }
        self.send_video_control(Control::TogglePause);
        true
    }

    pub(super) fn video_input(&mut self, message: u32, point: POINT) {
        let Some(view) = self.video.as_deref_mut().filter(|view| view.active) else {
            return;
        };
        let Some(panel) = &mut view.panel else { return };
        let Some(layout) = &panel.layout else { return };
        let hit = layout.hit(point);
        let chapters = view
            .media
            .as_ref()
            .map_or(&[][..], |media| media.chapters.as_slice());
        let fraction = || layout.seek_fraction(point.x, view.status.duration_ms, chapters);
        let mut click = None;
        let mut seek = None;
        match message {
            WM_LBUTTONDOWN => {
                panel.focus = None;
                panel.pressed = hit;
                if hit == Some(Hit::Seek) && view.status.seekable {
                    panel.drag = Some(fraction());
                }
            }
            WM_LBUTTONUP => {
                if panel.drag.take().is_some() {
                    seek = Some(fraction());
                } else if panel.pressed == hit
                    && let Some(Hit::Button(action)) = hit
                {
                    click = Some(action);
                }
                panel.pressed = None;
            }
            WM_CAPTURECHANGED => {
                panel.pressed = None;
                panel.drag = None;
            }
            _ => {}
        }
        if message != WM_CAPTURECHANGED {
            panel.reveal.pointer(true, true, Instant::now());
        }
        if let Some(fraction) = seek {
            let position = (view.status.duration_ms as f64 * f64::from(fraction)).round() as i64;
            self.close_video_menu();
            self.video_seek_to(position);
            return;
        }
        if let Some(action) = click {
            self.video_action(action);
        }
        self.render_video_panel();
    }

    pub(super) fn video_focus(&mut self, reverse: bool) {
        let language = self.lang();
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active)
            && let Some(mut panel) = view.panel.take()
        {
            let model = Self::video_model(
                view,
                &panel,
                language,
                self.fullscreen,
                self.catalog.files.len() > 1,
                self.update_download_busy,
                Instant::now(),
            );
            if let Some(layout) = &panel.layout {
                panel.focus = layout.focus(panel.focus, reverse, &model);
            }
            panel.hover = None;
            panel.seek_hover = None;
            panel.reveal.show();
            view.panel = Some(panel);
            self.render_video_panel();
        }
    }

    pub(super) fn video_activate_focus(&mut self) {
        if let Some(action) = self
            .video()
            .and_then(|view| view.panel.as_ref())
            .and_then(|panel| panel.focus)
        {
            self.video_action(action);
        }
    }

    pub(super) fn hide_video_panel(&mut self) {
        if let Some(panel) = self
            .video
            .as_deref_mut()
            .and_then(|view| view.panel.as_mut())
        {
            let (cursor, zone) = (panel.cursor, panel.zone);
            panel.clear_input();
            panel.cursor = cursor;
            panel.zone = zone;
            self.render_video_panel();
        }
    }

    pub(super) fn video_pause(&mut self) {
        if let Some(view) = self.video() {
            if view.failed() {
                self.handle_command(cmd::RELOAD);
            } else {
                self.send_video_control(Control::TogglePause);
                self.reveal_video_panel();
            }
        }
    }

    fn video_seek_to(&mut self, position: i64) {
        if let Some(view) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && view.status.seekable)
        {
            let position = position.clamp(0, view.status.duration_ms);
            let now = Instant::now();
            view.previous_position = Some(view.display_status(now).elapsed_ms);
            view.playhead.seek(position, now);
            self.send_video_control(Control::Seek(position));
            self.refresh_video_menu();
            self.reveal_video_panel();
        }
    }

    pub(super) fn video_seek_percent(&mut self, percent: u32) {
        if let Some(view) = self.video() {
            self.video_seek_to(view.status.duration_ms.saturating_mul(i64::from(percent)) / 100);
        }
    }

    pub(super) fn video_next_frame(&mut self) {
        if self.video().is_some_and(|view| {
            view.status.video_ready && matches!(view.status.phase, Phase::Playing | Phase::Paused)
        }) {
            self.send_video_control(Control::NextFrame);
            self.reveal_video_panel();
        }
    }

    pub(super) fn video_seek_relative(&mut self, delta_ms: i64) {
        if let Some(view) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active && view.status.seekable)
        {
            view.previous_position = Some(view.display_status(Instant::now()).elapsed_ms);
            self.send_video_control(Control::SeekBy(delta_ms));
            self.refresh_video_menu();
            self.reveal_video_panel();
        }
    }

    pub(super) fn video_audio(&mut self, delta: i32, toggle_mute: bool) {
        if let Some(view) = self.video() {
            let volume = view.preferences.volume.saturating_add(delta);
            let muted = toggle_mute && !view.preferences.muted;
            self.set_video_volume(volume, muted);
            self.reveal_video_volume();
        }
    }

    pub(super) fn video_wheel_at(&mut self, mut point: POINT, delta: i16) -> bool {
        if self.video_menu_wheel_at(point, delta) {
            return true;
        }
        let Some(panel) = self.video().and_then(|view| view.panel.as_ref()) else {
            return false;
        };
        let Some(volume) = &panel.volume else {
            return false;
        };
        let Some(layout) = volume.layout else {
            return false;
        };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut point);
        }
        let hot = panel
            .edges
            .is_some_and(|edges| edges.zone(point) == EdgeZone::Side);
        if hot || (volume.layer.visible && layout.contains(layout.local(point))) {
            if delta != 0 {
                self.video_audio(if delta > 0 { 5 } else { -5 }, false);
            }
            return true;
        }
        false
    }

    fn reveal_video_volume(&mut self) {
        if let Some(volume) = self
            .video
            .as_deref_mut()
            .filter(|view| view.active)
            .and_then(|view| view.panel.as_mut())
            .and_then(|panel| panel.volume.as_mut())
        {
            volume.reveal.show();
            self.render_video_panel();
        }
    }

    pub(super) fn video_volume_input(&mut self, message: u32, point: POINT) {
        let Some(view) = self.video.as_deref_mut().filter(|view| view.active) else {
            return;
        };
        let Some(volume) = view.panel.as_mut().and_then(|panel| panel.volume.as_mut()) else {
            return;
        };
        let Some(layout) = volume.layout else {
            return;
        };
        let hit = layout.hit(point);
        let mut value = None;
        let mut toggle = false;
        match message {
            WM_LBUTTONDOWN => {
                volume.pressed = hit;
                if hit == Some(VolumeHit::Track) {
                    value = Some(layout.value(point.y));
                }
            }
            WM_LBUTTONUP => match volume.pressed.take() {
                Some(VolumeHit::Track) => value = Some(layout.value(point.y)),
                Some(VolumeHit::Mute) if hit == Some(VolumeHit::Mute) => toggle = true,
                _ => {}
            },
            WM_CAPTURECHANGED => volume.pressed = None,
            _ => {}
        }
        if message != WM_CAPTURECHANGED {
            volume.reveal.pointer(true, true, Instant::now());
        }
        if let Some(value) = value {
            self.set_video_volume(value, false);
        } else if toggle {
            let preferences = view.preferences;
            self.set_video_volume(preferences.volume, !preferences.muted);
        }
        self.render_video_panel();
    }

    fn set_video_volume(&mut self, volume: i32, muted: bool) {
        let mut changed = false;
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            let volume = volume.clamp(0, MAX_VOLUME);
            if view.preferences.volume == volume && view.preferences.muted == muted {
                return;
            }
            view.preferences.volume = volume;
            view.preferences.muted = muted;
            self.send_video_control(Control::Audio { volume, muted });
            changed = true;
        }
        if changed {
            self.schedule_video_preferences_save();
        }
    }

    pub(super) fn video_loop(&mut self) {
        let mut changed = false;
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.preferences.looping = !view.preferences.looping;
            let looping = view.preferences.looping;
            self.send_video_control(Control::Loop(looping));
            self.reveal_video_panel();
            changed = true;
        }
        if changed {
            self.schedule_video_preferences_save();
        }
    }

    fn video_rate(&mut self, rate: f32) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.playhead
                .change_rate(view.status, view.preferences.rate, Instant::now());
            view.preferences.rate = rate;
            self.send_video_control(Control::Rate(rate));
            self.reveal_video_panel();
            self.schedule_video_preferences_save();
        }
    }

    pub(super) fn video_subtitle(&mut self, id: i32) {
        let mut changed = false;
        let mut track_to_apply = None;
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            track_to_apply = if id == -2 {
                let track = view
                    .media
                    .as_ref()
                    .and_then(|media| media.subtitles.iter().find(|track| track.id >= 0))
                    .map(|track| track.id);
                view.preferences.subtitle_track = -2;
                track
            } else {
                view.preferences.subtitle_track = id;
                Some(id)
            };
            changed = true;
        }
        if let Some(track) = track_to_apply {
            self.send_video_control(Control::SubtitleTrack(track));
        }
        if changed {
            self.schedule_video_preferences_save();
            self.refresh_video_menu();
            self.render_video_panel();
        }
    }

    fn schedule_video_preferences_save(&mut self) {
        let Some(preferences) = self.video().map(|view| view.preferences) else {
            return;
        };
        self.settings.video_volume = preferences.volume;
        self.settings.video_muted = preferences.muted;
        self.settings.video_looping = preferences.looping;
        self.settings.video_rate_percent = (preferences.rate * 100.0).round() as u32;
        self.settings.video_subtitle_track = preferences.subtitle_track;
        let timer = unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_VIDEO_PREFERENCES);
            SetTimer(Some(self.hwnd), TIMER_VIDEO_PREFERENCES, 450, None)
        };
        if timer == 0 {
            self.settings.save_video_preferences();
        }
    }

    fn send_video_control(&self, command: Control) {
        if let Some(player) = self.video().and_then(|view| view.playback.as_ref()) {
            player.control(self.generation, command);
        }
    }

    fn video_action(&mut self, action: Action) {
        if !matches!(action, Action::Rate | Action::Tracks | Action::More) {
            self.close_video_menu();
        }
        match action {
            Action::Previous => self.navigate(-1),
            Action::Play => self.video_pause(),
            Action::Next => self.navigate(1),
            Action::Volume => self.reveal_video_volume(),
            Action::Loop => self.video_loop(),
            Action::Rate | Action::Tracks | Action::More => self.video_menu(action),
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::Open => self.open_dialog(),
            Action::Update => self.begin_update_install(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_reveal_requires_a_zone_and_expires_while_stationary() {
        let now = Instant::now();
        let mut reveal = Reveal::default();
        reveal.pointer(false, true, now);
        assert!(!reveal.visible(now));

        reveal.pointer(true, true, now);
        assert!(reveal.visible(now));
        let idle = now + HIDE_AFTER;
        reveal.pointer(true, false, idle);
        assert!(!reveal.visible(idle));

        reveal.pointer(true, true, idle);
        assert!(reveal.visible(idle));
        reveal.pointer(false, true, idle);
        assert!(!reveal.visible(idle));
    }
}
