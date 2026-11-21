//! LibVLC is loaded on the playback thread when the first video opens.
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    os::windows::ffi::OsStrExt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
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
pub const MEDIA_ITEM_LIMIT: usize = 256;
const REFRESH_INTERVAL: Duration = Duration::from_millis(80);
const OPEN_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const OPEN_REFRESH_LIMIT: Duration = Duration::from_secs(1);

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
    pub audio_track: i32,
    pub subtitle_track: i32,
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
            audio_track: -1,
            subtitle_track: -1,
        }
    }
}

#[derive(Debug)]
pub struct Track {
    pub id: i32,
    pub name: String,
}

#[derive(Debug)]
pub struct Chapter {
    pub start_ms: i64,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct MediaDetails {
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
    pub chapters: Vec<Chapter>,
}

type PlaybackUpdate = (u64, PlaybackStatus, Option<Arc<MediaDetails>>);

pub enum Control {
    TogglePause,
    Seek(i64),
    SeekBy(i64),
    NextFrame,
    AudioTrack(i32),
    SubtitleTrack(i32),
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
    /// -2 keeps LibVLC's automatic track selection, -1 disables subtitles.
    pub subtitle_track: i32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            volume: 100,
            muted: false,
            looping: false,
            rate: 1.0,
            subtitle_track: -2,
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
    status: Arc<Mailbox<PlaybackUpdate>>,
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
                let mut details = None;
                let mut opening_until = None;
                loop {
                    let request = if player.is_some() {
                        let interval =
                            if opening_until.is_some_and(|deadline| Instant::now() < deadline) {
                                OPEN_REFRESH_INTERVAL
                            } else {
                                REFRESH_INTERVAL
                            };
                        match receiver.recv_timeout(interval) {
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
                            details = None;
                            opening_until = None;
                            match Player::open(player.take(), surface, &path, preferences) {
                                Ok(opened) => {
                                    player = Some(opened);
                                    audio_pending = true;
                                    opening_until = Some(Instant::now() + OPEN_REFRESH_LIMIT);
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
                            details = None;
                        }
                        Some(Request::Close) => break,
                        _ => {}
                    }
                    let mut details_changed = false;
                    if let Some(current) = &mut player {
                        let mut status = current.status();
                        if current.apply_pending_seek() {
                            status = current.status();
                        }
                        if details.is_none()
                            && status.video_ready
                            && matches!(status.phase, Phase::Playing | Phase::Paused)
                        {
                            let loaded = current.media_details();
                            let selected = current.preferences.subtitle_track;
                            if selected == -1
                                || loaded.subtitles.iter().any(|track| track.id == selected)
                            {
                                current.control(Control::SubtitleTrack(selected));
                            }
                            details = Some(Arc::new(loaded));
                            details_changed = true;
                        }
                        if status.dimensions.is_some()
                            || matches!(status.phase, Phase::Ended | Phase::Failed)
                        {
                            opening_until = None;
                        }
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
                        && (details_changed || previous != Some(status))
                        && generation == latest.load(Ordering::Acquire)
                        && inbox.publish((generation, status, details.clone()))
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

    pub fn take(&self) -> Option<PlaybackUpdate> {
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
    libvlc_media_player_next_frame: fn(Handle),
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
    libvlc_audio_get_track: fn(Handle) -> c_int,
    libvlc_audio_set_track: fn(Handle, c_int) -> c_int,
    libvlc_audio_get_track_description: fn(Handle) -> *mut TrackDescription,
    libvlc_video_get_spu: fn(Handle) -> c_int,
    libvlc_video_set_spu: fn(Handle, c_int) -> c_int,
    libvlc_video_get_spu_description: fn(Handle) -> *mut TrackDescription,
    libvlc_track_description_list_release: fn(*mut TrackDescription),
    libvlc_media_player_get_full_chapter_descriptions: fn(Handle, c_int, *mut *mut *mut ChapterDescription) -> c_int,
    libvlc_chapter_descriptions_release: fn(*mut *mut ChapterDescription, u32),
}

#[repr(C)]
struct TrackDescription {
    id: c_int,
    name: *const c_char,
    next: *const TrackDescription,
}

#[repr(C)]
struct ChapterDescription {
    start_ms: i64,
    _duration_ms: i64,
    name: *const c_char,
}

unsafe fn media_name(name: *const c_char) -> String {
    if name.is_null() {
        return String::new();
    }
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    String::from_utf8_lossy(&bytes[..bytes.len().min(640)])
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect()
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
    pending_seek_ms: Option<i64>,
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
                pending_seek_ms: None,
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
        current.pending_seek_ms = None;
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
                audio_track: (api.libvlc_audio_get_track)(player),
                subtitle_track: (api.libvlc_video_get_spu)(player),
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

    fn media_details(&self) -> MediaDetails {
        let api = &self.api;
        let player = self.player;
        let tracks = |head: *mut TrackDescription| {
            let mut tracks = Vec::new();
            let mut link = head.cast_const();
            unsafe {
                while let Some(track) = link.as_ref().filter(|_| tracks.len() < MEDIA_ITEM_LIMIT) {
                    tracks.push(Track {
                        id: track.id,
                        name: media_name(track.name),
                    });
                    link = track.next;
                }
                if !head.is_null() {
                    (api.libvlc_track_description_list_release)(head);
                }
            }
            tracks
        };
        let mut chapters = Vec::new();
        unsafe {
            let mut descriptions = std::ptr::null_mut();
            let count = (api.libvlc_media_player_get_full_chapter_descriptions)(
                player,
                -1,
                &mut descriptions,
            );
            if count > 0 && !descriptions.is_null() {
                let limit = (count as usize).min(MEDIA_ITEM_LIMIT);
                for &description in std::slice::from_raw_parts(descriptions, limit) {
                    if let Some(chapter) =
                        description.as_ref().filter(|chapter| chapter.start_ms >= 0)
                    {
                        chapters.push(Chapter {
                            start_ms: chapter.start_ms,
                            name: media_name(chapter.name),
                        });
                    }
                }
                (api.libvlc_chapter_descriptions_release)(descriptions, count as u32);
            }
            chapters.sort_by_key(|chapter| chapter.start_ms);
            chapters.dedup_by_key(|chapter| chapter.start_ms);
            MediaDetails {
                audio: tracks((api.libvlc_audio_get_track_description)(player)),
                subtitles: tracks((api.libvlc_video_get_spu_description)(player)),
                chapters,
            }
        }
    }

    fn apply_audio(&self) -> bool {
        unsafe {
            (self.api.libvlc_audio_set_mute)(self.player, i32::from(self.preferences.muted));
            (self.api.libvlc_audio_set_volume)(self.player, self.preferences.volume) == 0
        }
    }

    fn apply_pending_seek(&mut self) -> bool {
        let Some(position) = self.pending_seek_ms else {
            return false;
        };
        if !matches!(
            unsafe { (self.api.libvlc_media_player_get_state)(self.player) },
            3 | 4
        ) {
            return false;
        }
        unsafe { (self.api.libvlc_media_player_set_time)(self.player, position) };
        self.pending_seek_ms = None;
        true
    }

    fn restart(&self) -> bool {
        unsafe {
            (self.api.libvlc_media_player_stop)(self.player);
            (self.api.libvlc_media_player_set_rate)(self.player, self.preferences.rate);
            if (self.api.libvlc_media_player_play)(self.player) != 0 {
                return false;
            }
            self.apply_audio();
            if self.preferences.subtitle_track >= -1 {
                let _ =
                    (self.api.libvlc_video_set_spu)(self.player, self.preferences.subtitle_track);
            }
            true
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
                    let state = (api.libvlc_media_player_get_state)(player);
                    let seekable = (api.libvlc_media_player_is_seekable)(player) != 0;
                    if seekable {
                        let previous = (api.libvlc_media_player_get_time)(player).max(0);
                        let ended = matches!(state, 5 | 6);
                        if ended && !self.restart() {
                            return false;
                        }
                        let target = if matches!(action, Control::SeekBy(_)) {
                            (if ended { self.end_ms } else { previous }).saturating_add(ms)
                        } else {
                            ms
                        };
                        let target = target.clamp(0, self.end_ms);
                        if ended {
                            self.pending_seek_ms = Some(target);
                        } else {
                            (api.libvlc_media_player_set_time)(player, target);
                        }
                    }
                }
                Control::NextFrame => {
                    (api.libvlc_media_player_next_frame)(player);
                }
                Control::AudioTrack(id) => {
                    return (api.libvlc_audio_set_track)(player, id) == 0 && id >= 0;
                }
                Control::SubtitleTrack(id) => {
                    if (api.libvlc_video_set_spu)(player, id) == 0 {
                        self.preferences.subtitle_track = id;
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
