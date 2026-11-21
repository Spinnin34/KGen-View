//! LibVLC is loaded on the playback thread when the first video opens.
use std::{
    ffi::{CString, c_char, c_int, c_void},
    os::windows::ffi::OsStrExt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND, LPARAM, WPARAM},
        System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
            LoadLibraryExW,
        },
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
    core::{PCSTR, PCWSTR},
};

use crate::mailbox::Mailbox;

pub const WM_PLAYBACK: u32 = WM_APP + 11;
pub const WM_PLAYBACK_CLOSED: u32 = WM_APP + 12;
pub const MAX_VOLUME: i32 = 200;
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Playing,
    Paused,
    Ended,
    MissingEngine,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackStatus {
    pub phase: Phase,
    pub elapsed_ms: i64,
    pub duration_ms: i64,
    pub seekable: bool,
    pub video_ready: bool,
    pub dimensions: Option<(u32, u32)>,
}

impl PlaybackStatus {
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            elapsed_ms: 0,
            duration_ms: 0,
            seekable: false,
            video_ready: false,
            dimensions: None,
        }
    }
}

pub enum Control {
    TogglePause,
    Seek(i64),
    SeekBy(i64),
    Audio { volume: i32, muted: bool },
    Loop(bool),
    Rate(f32),
}

#[derive(Clone, Copy)]
pub struct Preferences {
    pub volume: i32,
    pub muted: bool,
    pub looping: bool,
    pub rate: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            volume: 100,
            muted: false,
            looping: false,
            rate: 1.0,
        }
    }
}

enum Request {
    Open {
        generation: u64,
        path: String,
        preferences: Preferences,
    },
    Input(u64, Control),
    Stop(u64),
    Close,
}

pub struct Playback {
    commands: Sender<Request>,
    latest_generation: Arc<AtomicU64>,
    status: Arc<Mailbox<(u64, PlaybackStatus)>>,
}

impl Playback {
    pub fn start(owner: HWND, surface: HWND) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let status = Arc::new(Mailbox::default());
        let latest_generation = Arc::new(AtomicU64::new(0));
        let inbox = status.clone();
        let latest = latest_generation.clone();
        let owner = owner.0 as usize;
        let surface = surface.0 as usize;
        thread::Builder::new()
            .name("video-playback".into())
            .spawn(move || {
                let owner = HWND(owner as *mut c_void);
                let surface = HWND(surface as *mut c_void);
                let mut player: Option<Player> = None;
                let mut generation = 0;
                let mut previous = None;
                let mut audio_pending = false;
                loop {
                    let request = if player.is_some() {
                        match receiver.recv_timeout(REFRESH_INTERVAL) {
                            Ok(request) => Some(request),
                            Err(RecvTimeoutError::Timeout) => None,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        let Ok(request) = receiver.recv() else { break };
                        Some(request)
                    };
                    let mut snapshot = None;
                    match request {
                        Some(Request::Open {
                            generation: requested,
                            path,
                            preferences,
                        }) if requested == latest.load(Ordering::Acquire) => {
                            generation = requested;
                            previous = None;
                            match Player::open(player.take(), surface, &path, preferences) {
                                Ok(opened) => {
                                    player = Some(opened);
                                    audio_pending = true;
                                }
                                Err(error) => snapshot = Some(PlaybackStatus::new(error)),
                            }
                        }
                        Some(Request::Input(requested, action))
                            if requested == generation
                                && requested == latest.load(Ordering::Acquire) =>
                        {
                            if let Some(player) = &mut player {
                                audio_pending |= player.control(action);
                            }
                        }
                        Some(Request::Stop(requested))
                            if requested == latest.load(Ordering::Acquire) =>
                        {
                            player = None;
                            previous = None;
                        }
                        Some(Request::Close) => break,
                        _ => {}
                    }
                    if let Some(current) = &mut player {
                        let mut status = current.status();
                        if status.phase == Phase::Ended
                            && current.preferences.looping
                            && generation == latest.load(Ordering::Acquire)
                        {
                            if current.restart() {
                                audio_pending = true;
                                status.phase = Phase::Opening;
                                status.elapsed_ms = 0;
                                status.seekable = false;
                            } else {
                                status.phase = Phase::Failed;
                            }
                        }
                        if audio_pending && matches!(status.phase, Phase::Playing | Phase::Paused) {
                            audio_pending = !current.apply_audio();
                        }
                        snapshot = Some(status);
                        if status.phase == Phase::Failed {
                            player = None;
                        }
                    }
                    if let Some(status) = snapshot
                        && previous != Some(status)
                        && generation == latest.load(Ordering::Acquire)
                        && inbox.publish((generation, status))
                    {
                        previous = Some(status);
                        unsafe {
                            let _ = PostMessageW(Some(owner), WM_PLAYBACK, WPARAM(0), LPARAM(0));
                        }
                    }
                }
                // Win32 must keep the render HWND alive until LibVLC has
                // finished stopping its video output.
                drop(player);
                unsafe {
                    let _ = PostMessageW(Some(owner), WM_PLAYBACK_CLOSED, WPARAM(0), LPARAM(0));
                }
            })?;
        Ok(Self {
            commands,
            latest_generation,
            status,
        })
    }

    pub fn open(&self, generation: u64, path: String, preferences: Preferences) {
        self.latest_generation.store(generation, Ordering::Release);
        let _ = self.commands.send(Request::Open {
            generation,
            path,
            preferences,
        });
    }

    pub fn control(&self, generation: u64, control: Control) {
        let _ = self.commands.send(Request::Input(generation, control));
    }

    pub fn stop(&self, generation: u64) {
        self.latest_generation.store(generation, Ordering::Release);
        let _ = self.commands.send(Request::Stop(generation));
    }

    pub fn take(&self) -> Option<(u64, PlaybackStatus)> {
        self.status.take()
    }

    pub fn close(&self) {
        self.latest_generation.fetch_add(1, Ordering::AcqRel);
        let _ = self.commands.send(Request::Close);
    }
}

struct Library(HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

// Signatures from the LibVLC 3 C API. All pointers are used on the playback
// thread and released before the DLL handle.
macro_rules! libvlc_api {
    ($($name:ident: fn($($arg:ty),*) $(-> $result:ty)?),+ $(,)?) => {
        struct Api {
            $($name: unsafe extern "C" fn($($arg),*) $(-> $result)?,)+
            _library: Library,
        }

        impl Api {
            fn load() -> Option<Self> {
                let path = std::env::current_exe().ok()?.parent()?.join("vlc").join("libvlc.dll");
                let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
                let library = Library(unsafe {
                    LoadLibraryExW(
                        PCWSTR(path.as_ptr()),
                        None,
                        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                    ).inspect_err(|error| report_failure("load LibVLC", error.code().0)).ok()?
                });
                Some(Self {
                    $($name: unsafe {
                        let symbol = std::ffi::CStr::from_bytes_with_nul(
                            concat!(stringify!($name), "\0").as_bytes()
                        ).ok()?;
                        std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            unsafe extern "C" fn($($arg),*) $(-> $result)?
                        >(GetProcAddress(library.0, PCSTR(symbol.as_ptr().cast())).or_else(|| {
                            report_failure(stringify!($name), 0);
                            None
                        })?)
                    },)+
                    _library: library,
                })
            }
        }
    };
}

type Handle = *mut c_void;

libvlc_api! {
    libvlc_new: fn(c_int, *const *const c_char) -> Handle,
    libvlc_release: fn(Handle),
    libvlc_media_new_path: fn(Handle, *const c_char) -> Handle,
    libvlc_media_release: fn(Handle),
    libvlc_media_tracks_get: fn(Handle, *mut *mut *const MediaTrack) -> u32,
    libvlc_media_tracks_release: fn(*mut *const MediaTrack, u32),
    libvlc_media_player_new: fn(Handle) -> Handle,
    libvlc_media_player_release: fn(Handle),
    libvlc_media_player_set_media: fn(Handle, Handle),
    libvlc_media_player_get_media: fn(Handle) -> Handle,
    libvlc_media_player_set_hwnd: fn(Handle, Handle),
    libvlc_video_set_mouse_input: fn(Handle, u32),
    libvlc_video_set_key_input: fn(Handle, u32),
    libvlc_media_player_play: fn(Handle) -> c_int,
    libvlc_media_player_stop: fn(Handle),
    libvlc_media_player_pause: fn(Handle),
    libvlc_media_player_get_state: fn(Handle) -> c_int,
    libvlc_media_player_get_time: fn(Handle) -> i64,
    libvlc_media_player_get_length: fn(Handle) -> i64,
    libvlc_media_player_set_time: fn(Handle, i64),
    libvlc_media_player_set_rate: fn(Handle, f32) -> c_int,
    libvlc_media_player_is_seekable: fn(Handle) -> c_int,
    libvlc_media_player_has_vout: fn(Handle) -> u32,
    libvlc_video_get_track: fn(Handle) -> c_int,
    libvlc_audio_set_volume: fn(Handle, c_int) -> c_int,
    libvlc_audio_set_mute: fn(Handle, c_int),
}

// Prefixes of the C structs in the LibVLC 3 media API. The details union
// is read as a video pointer only for tracks flagged as video.
#[repr(C)]
struct MediaTrack {
    _codec: u32,
    _original_codec: u32,
    id: c_int,
    kind: c_int,
    _profile: c_int,
    _level: c_int,
    video: *const VideoTrack,
}

#[repr(C)]
struct VideoTrack {
    height: u32,
    width: u32,
    sample_aspect_num: u32,
    sample_aspect_den: u32,
    _frame_rate_num: u32,
    _frame_rate_den: u32,
    orientation: c_int,
}

impl VideoTrack {
    fn display_dimensions(&self) -> Option<(u32, u32)> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let width = match (self.sample_aspect_num, self.sample_aspect_den) {
            (0, _) | (_, 0) => self.width,
            (num, den) => {
                let den = u64::from(den);
                let pixels = (u64::from(self.width) * u64::from(num) + den / 2) / den;
                pixels.clamp(1, u64::from(u32::MAX)) as u32
            }
        };
        // Transposed orientations exchange the two display axes.
        Some(if (4..=7).contains(&self.orientation) {
            (self.height, width)
        } else {
            (width, self.height)
        })
    }
}

struct Player {
    api: Api,
    instance: Handle,
    player: Handle,
    preferences: Preferences,
    end_ms: i64,
    had_output: bool,
    dimensions: Option<(u32, u32)>,
}

impl Player {
    fn create(surface: HWND) -> Result<Self, Phase> {
        let api = Api::load().ok_or(Phase::MissingEngine)?;
        let options = [
            c"--ignore-config",
            c"--no-video-title-show",
            c"--no-osd",
            c"--no-sub-autodetect-file",
            c"--file-caching=180",
            c"--avcodec-hw=any",
        ]
        .map(|option| option.as_ptr());
        unsafe {
            let instance = (api.libvlc_new)(options.len() as c_int, options.as_ptr());
            if instance.is_null() {
                report_failure("initialize LibVLC", 0);
                return Err(Phase::MissingEngine);
            }
            let player = (api.libvlc_media_player_new)(instance);
            if player.is_null() {
                report_failure("create LibVLC player", 0);
                (api.libvlc_release)(instance);
                return Err(Phase::Failed);
            }
            (api.libvlc_media_player_set_hwnd)(player, surface.0);
            (api.libvlc_video_set_mouse_input)(player, 0);
            (api.libvlc_video_set_key_input)(player, 0);
            Ok(Self {
                api,
                instance,
                player,
                preferences: Preferences::default(),
                end_ms: 0,
                had_output: false,
                dimensions: None,
            })
        }
    }

    fn open(
        previous: Option<Self>,
        surface: HWND,
        path: &str,
        preferences: Preferences,
    ) -> Result<Self, Phase> {
        let mut current = match previous {
            Some(player) => {
                unsafe { (player.api.libvlc_media_player_stop)(player.player) };
                player
            }
            None => Self::create(surface)?,
        };
        let path = CString::new(path).map_err(|_| Phase::Failed)?;
        current.preferences = Preferences {
            volume: preferences.volume.clamp(0, MAX_VOLUME),
            ..preferences
        };
        let api = &current.api;
        unsafe {
            let media = (api.libvlc_media_new_path)(current.instance, path.as_ptr());
            if media.is_null() {
                report_failure("create LibVLC media", 0);
                return Err(Phase::Failed);
            }
            (api.libvlc_media_player_set_media)(current.player, media);
            (api.libvlc_media_release)(media);
            (api.libvlc_media_player_set_rate)(current.player, preferences.rate);
            if (api.libvlc_media_player_play)(current.player) != 0 {
                report_failure("start LibVLC playback", 0);
                return Err(Phase::Failed);
            }
        }
        current.end_ms = 0;
        current.had_output = false;
        current.dimensions = None;
        current.apply_audio();
        Ok(current)
    }

    fn status(&mut self) -> PlaybackStatus {
        let api = &self.api;
        let player = self.player;
        unsafe {
            let elapsed = (api.libvlc_media_player_get_time)(player).max(0);
            let video_ready = (api.libvlc_media_player_has_vout)(player) != 0;
            if video_ready && self.dimensions.is_none() {
                self.dimensions = self.video_dimensions();
            }
            let duration = (api.libvlc_media_player_get_length)(player);
            if duration > 0 {
                self.end_ms = duration;
            }
            self.had_output |= video_ready || elapsed > 0;
            let phase = match (api.libvlc_media_player_get_state)(player) {
                3 => Phase::Playing,
                4 => Phase::Paused,
                5 | 6 if self.had_output || self.end_ms > 0 => Phase::Ended,
                // Some demuxers report end-of-file after finding no playable stream.
                6 | 7 => Phase::Failed,
                _ => Phase::Opening,
            };
            if phase == Phase::Failed {
                report_failure("decode video", 0);
            }
            PlaybackStatus {
                phase,
                elapsed_ms: if phase == Phase::Ended {
                    self.end_ms
                } else {
                    elapsed
                },
                duration_ms: self.end_ms,
                seekable: (api.libvlc_media_player_is_seekable)(player) != 0 && self.end_ms > 0,
                video_ready,
                dimensions: self.dimensions,
            }
        }
    }

    fn video_dimensions(&self) -> Option<(u32, u32)> {
        const VIDEO_TRACK: c_int = 1;
        let api = &self.api;
        unsafe {
            let media = (api.libvlc_media_player_get_media)(self.player);
            if media.is_null() {
                return None;
            }
            let mut tracks = std::ptr::null_mut();
            let count = (api.libvlc_media_tracks_get)(media, &mut tracks);
            (api.libvlc_media_release)(media);
            if tracks.is_null() {
                return None;
            }
            let selected = (api.libvlc_video_get_track)(self.player);
            let mut videos = std::slice::from_raw_parts(tracks, count as usize)
                .iter()
                .filter_map(|&track| track.as_ref())
                .filter(|track| track.kind == VIDEO_TRACK);
            let track = videos
                .clone()
                .find(|track| track.id == selected)
                .or_else(|| videos.next());
            let dimensions = track
                .and_then(|track| track.video.as_ref())
                .and_then(VideoTrack::display_dimensions);
            (api.libvlc_media_tracks_release)(tracks, count);
            dimensions
        }
    }

    fn apply_audio(&self) -> bool {
        unsafe {
            (self.api.libvlc_audio_set_mute)(self.player, i32::from(self.preferences.muted));
            (self.api.libvlc_audio_set_volume)(self.player, self.preferences.volume) == 0
        }
    }

    fn restart(&self) -> bool {
        unsafe {
            (self.api.libvlc_media_player_stop)(self.player);
            (self.api.libvlc_media_player_set_rate)(self.player, self.preferences.rate);
            (self.api.libvlc_media_player_play)(self.player) == 0
        }
    }

    fn control(&mut self, action: Control) -> bool {
        let api = &self.api;
        let player = self.player;
        unsafe {
            match action {
                Control::TogglePause => match (api.libvlc_media_player_get_state)(player) {
                    5 | 6 => return self.restart(),
                    3 | 4 => (api.libvlc_media_player_pause)(player),
                    _ => {}
                },
                Control::Seek(ms) | Control::SeekBy(ms) => {
                    if (api.libvlc_media_player_is_seekable)(player) != 0 {
                        let target = if matches!(action, Control::SeekBy(_)) {
                            (api.libvlc_media_player_get_time)(player)
                                .max(0)
                                .saturating_add(ms)
                        } else {
                            ms
                        };
                        (api.libvlc_media_player_set_time)(player, target.clamp(0, self.end_ms));
                    }
                }
                Control::Audio { volume, muted } => {
                    self.preferences.volume = volume.clamp(0, MAX_VOLUME);
                    self.preferences.muted = muted;
                    return !self.apply_audio();
                }
                Control::Loop(looping) => self.preferences.looping = looping,
                Control::Rate(rate) => {
                    if (api.libvlc_media_player_set_rate)(player, rate) == 0 {
                        self.preferences.rate = rate;
                    }
                }
            }
        }
        false
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            (self.api.libvlc_media_player_stop)(self.player);
            (self.api.libvlc_media_player_release)(self.player);
            (self.api.libvlc_release)(self.instance);
        }
    }
}

pub fn report_failure(operation: &str, code: i32) {
    if crate::diagnostics::enabled() {
        eprintln!("KGen View playback: {operation} failed ({code})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_size_accounts_for_sample_aspect_and_rotation() {
        let mut track = VideoTrack {
            width: 720,
            height: 576,
            sample_aspect_num: 16,
            sample_aspect_den: 15,
            _frame_rate_num: 0,
            _frame_rate_den: 0,
            orientation: 0,
        };
        assert_eq!(track.display_dimensions(), Some((768, 576)));
        track.orientation = 6;
        assert_eq!(track.display_dimensions(), Some((576, 768)));
    }
}
