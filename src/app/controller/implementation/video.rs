use super::*;
use crate::{
    platform::playback::{Control, Phase, Playback, PlaybackStatus, Preferences, report_failure},
    ui::video::{Action, Hit, Layer, Model, Painter, PanelLayout, RATES, Slider, rate_label},
};
use windows::Win32::UI::{
    HiDpi::GetDpiForWindow,
    WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GA_ROOTOWNER, GetAncestor, GetCursorPos, HMENU,
        MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, SW_HIDE, TPM_BOTTOMALIGN, TPM_NONOTIFY,
        TPM_RETURNCMD, TPM_RIGHTALIGN, TrackPopupMenu, WM_CAPTURECHANGED, WS_CHILD, WS_DISABLED,
        WS_EX_NOPARENTNOTIFY, WindowFromPoint,
    },
};

const SS_BLACKRECT: WINDOW_STYLE = WINDOW_STYLE(4);
pub(super) const TIMER_VIDEO_PANEL: usize = 9;
const POINTER_INTERVAL_MS: u32 = 90;
const HIDE_AFTER: std::time::Duration = std::time::Duration::from_millis(2700);

pub(in crate::app::controller) struct VideoView {
    surface: HWND,
    playback: Option<Playback>,
    active: bool,
    pub(super) closing: bool,
    status: PlaybackStatus,
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
    cursor: Option<POINT>,
    reveal_until: Instant,
    hover: Option<Hit>,
    over_panel: bool,
    focus: Option<Action>,
    pressed: Option<Hit>,
    drag: Option<(Slider, f32)>,
    menu_open: bool,
}

impl VideoPanel {
    fn new(layer: Layer) -> Self {
        Self {
            layer,
            painter: None,
            layout: None,
            cursor: None,
            reveal_until: Instant::now() + HIDE_AFTER,
            hover: None,
            over_panel: false,
            focus: None,
            pressed: None,
            drag: None,
            menu_open: false,
        }
    }

    fn reveal(&mut self) {
        self.reveal_until = Instant::now() + HIDE_AFTER;
    }

    fn clear_input(&mut self) {
        self.hover = None;
        self.over_panel = false;
        self.focus = None;
        self.pressed = None;
        self.drag = None;
    }

    fn wants_visible(&self, pinned: bool) -> bool {
        pinned
            || Instant::now() < self.reveal_until
            || self.over_panel
            || self.focus.is_some()
            || self.pressed.is_some()
            || self.menu_open
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
        let panel = Layer::new(hwnd, factory)
            .inspect_err(|error| report_failure("create floating controls", error.code().0))
            .ok()
            .map(VideoPanel::new);
        Ok(Self {
            surface,
            playback,
            panel,
            active: false,
            closing: false,
            status: PlaybackStatus::new(Phase::Opening),
            started: None,
            resize_pending: false,
            preferences: Preferences::default(),
            notice: None,
        })
    }

    fn failed(&self) -> bool {
        matches!(self.status.phase, Phase::Failed | Phase::MissingEngine)
    }

    fn controls_pinned(&self) -> bool {
        self.failed()
            || self.notice.is_some()
            || self.status.phase == Phase::Ended
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
        if let Some(panel) = &mut view.panel {
            panel.clear_input();
            panel.reveal();
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    TIMER_VIDEO_PANEL,
                    POINTER_INTERVAL_MS,
                    None,
                );
            }
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
            if let Some(panel) = &mut view.panel {
                panel.layer.hide();
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
        view.status = status;
        if was_failed != view.failed() {
            self.layout_video();
        } else {
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
    ) -> Model<'a> {
        Model {
            status: view.status,
            preferences: view.preferences,
            language,
            fullscreen,
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
        let foreground =
            unsafe { GetForegroundWindow() == self.hwnd && !IsIconic(self.hwnd).as_bool() };
        let show = !view.closing && foreground && panel.wants_visible(view.controls_pinned());
        if show && let Some(layout) = &panel.layout {
            let model = Self::video_model(
                view,
                &panel,
                language,
                self.fullscreen,
                self.catalog.files.len() > 1,
                self.update_download_busy,
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
            }
        } else {
            panel.layer.hide();
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
        let Some(panel) = &mut view.panel else {
            return;
        };
        let mut cursor = POINT::default();
        let foreground = unsafe { GetForegroundWindow() == self.hwnd };
        if !foreground {
            panel.clear_input();
            panel.cursor = None;
            panel.layer.hide();
            return;
        }
        let inside = unsafe {
            GetCursorPos(&mut cursor).is_ok()
                && GetAncestor(WindowFromPoint(cursor), GA_ROOTOWNER) == self.hwnd
        };
        let moved = panel.cursor != Some(cursor);
        panel.cursor = Some(cursor);
        let before = (panel.hover, panel.drag);
        if let Some(layout) = &panel.layout {
            unsafe {
                let _ = ScreenToClient(self.hwnd, &mut cursor);
            }
            cursor.x -= layout.offset.x;
            cursor.y -= layout.offset.y;
            panel.over_panel = inside && panel.layer.visible && layout.contains(cursor);
            panel.hover = if panel.over_panel {
                layout.hit(cursor)
            } else {
                None
            };
            if let Some((slider, _)) = panel.drag {
                panel.drag = Some((slider, layout.fraction(slider, cursor.x)));
            }
        }
        if moved && inside {
            panel.focus = None;
            panel.reveal();
        }
        if (moved && inside && !panel.layer.visible)
            || before != (panel.hover, panel.drag)
            || (panel.layer.visible && !panel.wants_visible(pinned))
        {
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
            panel.reveal();
            self.render_video_panel();
        }
    }

    pub(super) fn video_canvas_click(&mut self) -> bool {
        if self.video().is_none() {
            return false;
        }
        self.video_pause();
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
        let mut slider = None;
        match message {
            WM_LBUTTONDOWN => {
                panel.focus = None;
                panel.pressed = hit;
                if let Some(Hit::Slider(kind)) = hit
                    && (kind != Slider::Position || view.status.seekable)
                {
                    panel.drag = Some((kind, layout.fraction(kind, point.x)));
                }
            }
            WM_LBUTTONUP => {
                if let Some((kind, _)) = panel.drag.take() {
                    slider = Some((kind, layout.fraction(kind, point.x)));
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
        panel.reveal();
        let duration_ms = view.status.duration_ms;
        if let Some((kind, fraction)) = slider {
            match kind {
                Slider::Position => {
                    self.send_video_control(Control::Seek((duration_ms as f32 * fraction) as i64))
                }
                Slider::Volume => self.set_video_volume((fraction * 100.0).round() as i32, false),
            }
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
            );
            if let Some(layout) = &panel.layout {
                panel.focus = layout.focus(panel.focus, reverse, &model);
            }
            panel.hover = None;
            panel.reveal();
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
            panel.clear_input();
            panel.reveal_until = Instant::now();
            panel.layer.hide();
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
        }
    }

    fn set_video_volume(&mut self, volume: i32, muted: bool) {
        if let Some(view) = self.video.as_deref_mut().filter(|view| view.active) {
            let volume = volume.clamp(0, 100);
            view.preferences.volume = volume;
            view.preferences.muted = muted;
            self.send_video_control(Control::Audio { volume, muted });
            self.reveal_video_panel();
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
            Action::Mute => self.video_audio(0, true),
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
