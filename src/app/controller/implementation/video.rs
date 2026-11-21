use super::*;
use crate::{
    platform::playback::{
        Control, MAX_VOLUME, Phase, Playback, PlaybackStatus, Preferences, report_failure,
    },
    ui::video::{
        Action, EdgeLayout, EdgeZone, Hit, Layer, LayerKind, Model, Painter, PanelLayout,
        ProgressLine, RATES, VolumeHit, VolumeLayout, VolumePainter, rate_label,
    },
};
use windows::Win32::UI::{
    HiDpi::GetDpiForWindow,
    WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GA_ROOTOWNER, GetAncestor, GetCursorPos, HMENU,
        IsWindowVisible, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, SW_HIDE, TPM_BOTTOMALIGN,
        TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN, TrackPopupMenu, WM_CAPTURECHANGED, WS_CHILD,
        WS_DISABLED, WS_EX_NOPARENTNOTIFY, WindowFromPoint,
    },
};

mod clock;
use clock::Playhead;

const SS_BLACKRECT: WINDOW_STYLE = WINDOW_STYLE(4);
pub(super) const TIMER_VIDEO_PANEL: usize = 9;
const ACTIVE_INTERVAL_MS: u32 = 15;
const PROGRESS_INTERVAL_MS: u32 = 33;
const IDLE_INTERVAL_MS: u32 = 70;
const HIDE_AFTER: std::time::Duration = std::time::Duration::from_millis(2400);

pub(in crate::app::controller) struct VideoView {
    surface: HWND,
    playback: Option<Playback>,
    active: bool,
    pub(super) closing: bool,
    status: PlaybackStatus,
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
    focus: Option<Action>,
    pressed: Option<Hit>,
    drag: Option<f32>,
    menu_open: bool,
    timer_interval: u32,
    timeline_key: Option<(i32, i64)>,
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
            focus: None,
            pressed: None,
            drag: None,
            menu_open: false,
            timer_interval: 0,
            timeline_key: None,
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
        self.focus = None;
        self.pressed = None;
        self.drag = None;
        self.zone = EdgeZone::None;
        self.cursor = None;
        self.reveal = Reveal::default();
        if let Some(volume) = &mut self.volume {
            volume.clear_input();
        }
    }

    fn wants_visible(&self, pinned: bool, now: Instant) -> bool {
        pinned
            || self.reveal.visible(now)
            || self.focus.is_some()
            || self.pressed.is_some()
            || self.menu_open
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
            return;
        };
        view.active = true;
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
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub(super) fn stop_video(&mut self) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.active = false;
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
            }
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_VIDEO_PANEL);
                let _ = ShowWindow(view.surface, SW_HIDE);
            }
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
                if let Some(progress) = &mut panel.progress {
                    progress.hide();
                }
                if let Some(volume) = &mut panel.volume {
                    volume.layer.hide();
                }
            }
            playback.close();
        }
        true
    }

    pub(super) fn layout_video(&mut self) {
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
        self.schedule_video_resize();
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
        view.status = status;
        if was_failed != view.failed() {
            self.layout_video();
        } else if state_changed
            || view
                .panel
                .as_ref()
                .is_some_and(|panel| panel.timeline_changed(view.display_status(now)))
        {
            self.render_video_panel();
        }
        if new_size {
            self.schedule_video_resize();
        }
    }

    fn schedule_video_resize(&self) {
        if self.settings.auto_resize
            && self
                .video()
                .is_some_and(|view| view.resize_pending && view.status.dimensions.is_some())
        {
            self.schedule_auto_resize();
        }
    }

    pub(super) fn take_video_resize(&mut self) -> Option<(u32, u32)> {
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
            focus: panel.focus,
            drag: panel.drag,
            notice: view.notice.as_deref(),
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
        let foreground = allowed && unsafe { GetForegroundWindow() == self.hwnd };
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
        if let Some(volume) = &mut panel.volume {
            if foreground && volume.wants_visible(now) {
                if let Err(error) = volume.draw(self.hwnd, view.preferences) {
                    report_failure("draw volume controls", error.code().0);
                    volume.layer.hide();
                }
            } else {
                volume.layer.hide();
            }
        }
        if let Some(progress) = &mut panel.progress {
            let show = allowed && !panel.layer.visible && !view.failed();
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
        let Some(panel) = &mut view.panel else {
            return;
        };
        let foreground = unsafe { GetForegroundWindow() == self.hwnd };
        if !foreground {
            let visible = panel.layer.visible
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
        if zone != EdgeZone::None
            && (moved_screen || edge != panel.zone)
            && unsafe { GetAncestor(WindowFromPoint(screen), GA_ROOTOWNER) != self.hwnd }
        {
            zone = EdgeZone::None;
        }
        let moved = moved_screen || panel.zone != edge;
        panel.cursor = Some(screen);
        panel.zone = edge;
        panel.reveal.pointer(zone == EdgeZone::Bottom, moved, now);
        let before = (panel.hover, panel.drag, panel.focus);
        if moved && zone == EdgeZone::Bottom {
            panel.focus = None;
        }
        if let Some((layout, local)) = panel.layout.as_ref().zip(bottom_local) {
            panel.hover = (zone == EdgeZone::Bottom && panel.layer.visible)
                .then(|| layout.hit(local))
                .flatten();
            if panel.drag.is_some() {
                panel.drag = Some(layout.fraction(local.x));
            }
        }
        let mut redraw = (panel.wants_visible(pinned, now) && panel.layout.is_some())
            != panel.layer.visible
            || before != (panel.hover, panel.drag, panel.focus)
            || panel.timeline_changed(status);
        let mut set_volume = None;
        if let Some(volume) = &mut panel.volume
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
        if let Some(value) = set_volume {
            self.set_video_volume(value, false);
        }
        if redraw {
            self.render_video_panel();
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
        let mut click = None;
        let mut seek = None;
        match message {
            WM_LBUTTONDOWN => {
                panel.focus = None;
                panel.pressed = hit;
                if hit == Some(Hit::Seek) && view.status.seekable {
                    panel.drag = Some(layout.fraction(point.x));
                }
            }
            WM_LBUTTONUP => {
                if panel.drag.take().is_some() {
                    seek = Some(layout.fraction(point.x));
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
            view.playhead.seek(position, Instant::now());
            self.send_video_control(Control::Seek(position));
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

    pub(super) fn video_seek_relative(&mut self, delta_ms: i64) {
        if self.video().is_some_and(|view| view.status.seekable) {
            self.send_video_control(Control::SeekBy(delta_ms));
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
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            let volume = volume.clamp(0, MAX_VOLUME);
            if view.preferences.volume == volume && view.preferences.muted == muted {
                return;
            }
            view.preferences.volume = volume;
            view.preferences.muted = muted;
            self.send_video_control(Control::Audio { volume, muted });
        }
    }

    pub(super) fn video_loop(&mut self) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.preferences.looping = !view.preferences.looping;
            let looping = view.preferences.looping;
            self.send_video_control(Control::Loop(looping));
            self.reveal_video_panel();
        }
    }

    fn video_rate(&mut self, rate: f32) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            view.playhead
                .change_rate(view.status, view.preferences.rate, Instant::now());
            view.preferences.rate = rate;
            self.send_video_control(Control::Rate(rate));
            self.reveal_video_panel();
        }
    }

    fn send_video_control(&self, command: Control) {
        if let Some(player) = self.video().and_then(|view| view.playback.as_ref()) {
            player.control(self.generation, command);
        }
    }

    fn video_action(&mut self, action: Action) {
        match action {
            Action::Previous => self.navigate(-1),
            Action::Play => self.video_pause(),
            Action::Next => self.navigate(1),
            Action::Volume => self.reveal_video_volume(),
            Action::Loop => self.video_loop(),
            Action::Rate | Action::More => self.video_menu(action),
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::Open => self.open_dialog(),
            Action::Update => self.begin_update_install(),
        }
    }

    fn video_menu(&mut self, action: Action) {
        const RATE_BASE: usize = 3000;
        const LOOP: usize = 3010;
        const MUTE: usize = 3011;
        const BACK: usize = 3012;
        const FORWARD: usize = 3013;
        const KGEN_MENU: usize = 3014;
        const VOLUME_DOWN: usize = 3015;
        const VOLUME_UP: usize = 3016;
        let Some(view) = self.video() else { return };
        let Some(panel) = &view.panel else { return };
        let Some(mut anchor) = panel
            .layout
            .as_ref()
            .and_then(|layout| layout.anchor(action))
        else {
            return;
        };
        let spanish = self.lang() == Language::Spanish;
        let label = |es, en| if spanish { es } else { en };
        let Ok(menu) = PopupMenu::new() else { return };
        let preferences = view.preferences;
        let speed_items = |menu: &PopupMenu| {
            for (index, rate) in RATES.into_iter().enumerate() {
                menu.item(
                    RATE_BASE + index,
                    &rate_label(rate, self.lang()),
                    rate == preferences.rate,
                );
            }
        };
        if action == Action::Rate {
            speed_items(&menu);
        } else {
            menu.item(cmd::OPEN, label("Abrir archivo…", "Open file…"), false);
            menu.item(
                cmd::OPEN_FOLDER,
                label("Mostrar en el Explorador", "Show in Explorer"),
                false,
            );
            menu.item(cmd::COPY_PATH, label("Copiar ruta", "Copy path"), false);
            menu.separator();
            menu.item(
                BACK,
                label("Retroceder 10 segundos", "Back 10 seconds"),
                false,
            );
            menu.item(
                FORWARD,
                label("Avanzar 10 segundos", "Forward 10 seconds"),
                false,
            );
            menu.item(
                LOOP,
                label("Repetir en bucle", "Loop video"),
                preferences.looping,
            );
            menu.item(MUTE, label("Silenciar", "Mute"), preferences.muted);
            menu.item(VOLUME_DOWN, label("Bajar volumen", "Volume down"), false);
            menu.item(VOLUME_UP, label("Subir volumen", "Volume up"), false);
            if let Ok(speed) = PopupMenu::new() {
                speed_items(&speed);
                menu.submenu(speed, label("Velocidad", "Playback speed"));
            }
            menu.separator();
            menu.item(
                KGEN_MENU,
                label("Menú de KGen View", "KGen View menu"),
                false,
            );
        }
        if let Some(panel) = self
            .video
            .as_deref_mut()
            .and_then(|view| view.panel.as_mut())
        {
            panel.menu_open = true;
        }
        let command = unsafe {
            let _ = ClientToScreen(self.hwnd, &mut anchor);
            TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN | TPM_RIGHTALIGN,
                anchor.x,
                anchor.y,
                Some(0),
                self.hwnd,
                None,
            )
            .0 as usize
        };
        if let Some(panel) = self
            .video
            .as_deref_mut()
            .and_then(|view| view.panel.as_mut())
        {
            panel.menu_open = false;
        }
        match command {
            rate if (RATE_BASE..RATE_BASE + RATES.len()).contains(&rate) => {
                self.video_rate(RATES[rate - RATE_BASE])
            }
            LOOP => self.video_loop(),
            MUTE => self.video_audio(0, true),
            VOLUME_DOWN => self.video_audio(-5, false),
            VOLUME_UP => self.video_audio(5, false),
            BACK => self.video_seek_relative(-10_000),
            FORWARD => self.video_seek_relative(10_000),
            KGEN_MENU => {
                if let Some(menu) = &self.menu {
                    unsafe {
                        menu.show_all(self.hwnd, anchor.x - 135, anchor.y - 140);
                    }
                }
            }
            0 => {}
            other => self.handle_command(other),
        }
        self.reveal_video_panel();
    }
}

struct PopupMenu(HMENU);

impl PopupMenu {
    fn new() -> Result<Self, WinError> {
        Ok(Self(unsafe { CreatePopupMenu()? }))
    }

    fn item(&self, command: usize, text: &str, checked: bool) {
        let flags = if checked {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        unsafe {
            let _ = AppendMenuW(self.0, flags, command, PCWSTR(wide(text).as_ptr()));
        }
    }

    fn separator(&self) {
        unsafe {
            let _ = AppendMenuW(self.0, MF_SEPARATOR, 0, PCWSTR::null());
        }
    }

    fn submenu(&self, menu: Self, text: &str) {
        if unsafe {
            AppendMenuW(
                self.0,
                MF_POPUP,
                menu.0.0 as usize,
                PCWSTR(wide(text).as_ptr()),
            )
        }
        .is_ok()
        {
            std::mem::forget(menu);
        }
    }
}

impl Drop for PopupMenu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
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
