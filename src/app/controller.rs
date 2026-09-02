use std::{
    collections::HashSet,
    ffi::c_void,
    fs,
    mem::size_of,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Instant, SystemTime},
};

use windows::{
    Win32::{
        Foundation::{FILETIME, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SYSTEMTIME, WPARAM},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
                    D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
                D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1CreateFactory,
                ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::{
                BeginPaint, ClientToScreen, EndPaint, GetMonitorInfoW, InvalidateRect,
                MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT,
                UpdateWindow,
            },
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            DataExchange::COPYDATASTRUCT,
            LibraryLoader::GetModuleHandleW,
            Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
        },
        UI::{
            Controls::Dialogs::{
                GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
                OFN_HIDEREADONLY, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
            },
            Input::KeyboardAndMouse::{
                GetKeyState, ReleaseCapture, SetCapture, VK_ADD, VK_CONTROL, VK_DELETE, VK_END,
                VK_ESCAPE, VK_F5, VK_F9, VK_F11, VK_HOME, VK_LEFT, VK_NEXT, VK_OEM_MINUS,
                VK_OEM_PLUS, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_SUBTRACT,
            },
            Shell::{
                DragAcceptFiles, DragFinish, DragQueryFileW, FO_DELETE, FOF_ALLOWUNDO,
                FOF_NOERRORUI, SHFILEOPSTRUCTW, SHFileOperationW, ShellExecuteW,
            },
            WindowsAndMessaging::{
                AdjustWindowRectEx, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW,
                GWL_STYLE, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
                GetWindowRect, IDC_ARROW, IDI_APPLICATION, IsIconic, KillTimer, LoadCursorW,
                LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
                MB_YESNOCANCEL, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassExW,
                SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER,
                SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_COPYDATA, WM_CREATE, WM_DESTROY,
                WM_DPICHANGED, WM_DRAWITEM, WM_DROPFILES, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MEASUREITEM,
                WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETTINGCHANGE,
                WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_POPUP,
                WS_VISIBLE,
            },
        },
    },
    core::{Error as WinError, PCWSTR, PWSTR, w},
};

use crate::{
    catalog::FolderCatalog,
    commands as cmd,
    i18n::{Language, Text, tr},
    imaging::{ImageDocument, ImageError, wide_path},
    platform::{
        clipboard::{copy_bgra_image, copy_text},
        imgbb,
        menu::ThemedMenu,
        properties_dialog::{PropertiesData, PropertiesEdit},
        text::{format_bytes, nul_pos, wide, wide_with_nuls},
        theme::apply_window_theme,
    },
    settings::{Settings, ThemeMode},
    ui::{Chrome, layout::ChromeLayout, status::StatusModel, theme::Palette},
};

mod loader;
use loader::{CatalogService, DecodeService};

const CLASS_NAME: PCWSTR = w!("KGenView.MainWindow");
const WM_LOAD_DONE: u32 = WM_APP + 1;
const WM_PREFETCH_DONE: u32 = WM_APP + 2;
const WM_UPLOAD_DONE: u32 = WM_APP + 3;
const WM_CATALOG_DONE: u32 = WM_APP + 4;
const IMAGE_HANDOFF_DATA: usize = 1;
const TIMER_ANIMATION: usize = 1;
const TIMER_SLIDESHOW: usize = 2;
const TIMER_FOLDER_REFRESH: usize = 3;
const TIMER_AUTO_RESIZE: usize = 4;
const FOLDER_REFRESH_INTERVAL_MS: u32 = 5_000;
const AUTO_RESIZE_DELAY_MS: u32 = 120;
#[derive(Clone, Copy, Debug)]
enum ZoomMode {
    Fit,
    FitWidth,
    Actual,
    Custom,
}

struct LoadPayload {
    generation: u64,
    path: PathBuf,
    result: Result<ImageDocument, ImageError>,
    catalog: FolderCatalog,
}

struct PrefetchPayload {
    generation: u64,
    path: PathBuf,
    result: Result<ImageDocument, ImageError>,
}

struct CatalogPayload {
    generation: u64,
    path: PathBuf,
    catalog: FolderCatalog,
}

struct UploadPayload {
    result: Result<String, String>,
}

struct AppState {
    hwnd: HWND,
    settings: Settings,
    document: Option<ImageDocument>,
    active_path: Option<PathBuf>,
    catalog: FolderCatalog,
    cache: Vec<(PathBuf, ImageDocument)>,
    cache_bytes: usize,
    catalog_stamp: Option<SystemTime>,
    generation: u64,
    prefetch_enabled: bool,
    navigation_direction: isize,
    alive: Arc<AtomicBool>,
    loader: DecodeService,
    catalog_loader: CatalogService,
    d2d_factory: ID2D1Factory,
    target: Option<ID2D1HwndRenderTarget>,
    bitmap: Option<ID2D1Bitmap>,
    chrome: Chrome,
    status_model: StatusModel,
    menu: Option<ThemedMenu>,
    zoom_mode: ZoomMode,
    zoom: f32,
    center: (f32, f32),
    selection_start: Option<POINT>,
    selection_end: Option<POINT>,
    crop_selection: Option<(u32, u32, u32, u32)>,
    middle_start: Option<POINT>,
    fullscreen: bool,
    saved_rect: RECT,
    saved_style: WINDOW_STYLE,
    slideshow: bool,
    animation_playing: bool,
    failed_paths: HashSet<PathBuf>,
    load_started: Option<Instant>,
    load_time_ms: u128,
    uploading: bool,
}

impl AppState {
    fn new(settings: Settings) -> Result<Self, WinError> {
        let d2d_factory =
            unsafe { D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let alive = Arc::new(AtomicBool::new(true));
        Ok(Self {
            hwnd: HWND::default(),
            settings,
            document: None,
            active_path: None,
            catalog: FolderCatalog::default(),
            cache: Vec::new(),
            cache_bytes: 0,
            catalog_stamp: None,
            generation: 0,
            prefetch_enabled: false,
            navigation_direction: 1,
            alive: alive.clone(),
            loader: DecodeService::new(alive.clone()),
            catalog_loader: CatalogService::new(alive),
            d2d_factory,
            target: None,
            bitmap: None,
            chrome: Chrome::new(),
            status_model: StatusModel::default(),
            menu: None,
            zoom_mode: ZoomMode::Fit,
            zoom: 1.0,
            center: (0.0, 0.0),
            selection_start: None,
            selection_end: None,
            crop_selection: None,
            middle_start: None,
            fullscreen: false,
            saved_rect: RECT::default(),
            saved_style: WS_OVERLAPPEDWINDOW,
            slideshow: false,
            animation_playing: true,
            failed_paths: HashSet::new(),
            load_started: None,
            load_time_ms: 0,
            uploading: false,
        })
    }

    fn lang(&self) -> Language {
        self.settings.language
    }

    unsafe fn create_controls(&mut self) {
        unsafe {
            self.rebuild_menu();
        }
        unsafe { DragAcceptFiles(self.hwnd, true) };
        unsafe {
            let _ = SetTimer(
                Some(self.hwnd),
                TIMER_FOLDER_REFRESH,
                FOLDER_REFRESH_INTERVAL_MS,
                None,
            );
        }
        unsafe {
            self.apply_theme();
        }
        self.update_status(None);
    }

    unsafe fn apply_theme(&self) {
        unsafe { apply_window_theme(self.hwnd, self.settings.is_dark()) };
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }

    fn chrome_layout(&self) -> ChromeLayout {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        ChromeLayout::calculate(self.hwnd, rect, self.fullscreen)
    }

    unsafe fn ensure_target(&mut self) -> Result<(), WinError> {
        if self.target.is_none() {
            let mut client = RECT::default();
            unsafe { GetClientRect(self.hwnd, &mut client)? };
            let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: client.right.max(1) as u32,
                    height: client.bottom.max(1) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            self.target = Some(unsafe {
                self.d2d_factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES::default(),
                    &hwnd_properties,
                )?
            });
        }
        Ok(())
    }

    unsafe fn ensure_bitmap(&mut self) -> Result<(), WinError> {
        if self.bitmap.is_some() || self.document.is_none() {
            return Ok(());
        }
        unsafe { self.ensure_target()? };
        let frame = self.document.as_ref().unwrap().current_frame();
        let properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        self.bitmap = Some(unsafe {
            self.target.as_ref().unwrap().CreateBitmap(
                D2D_SIZE_U {
                    width: frame.width,
                    height: frame.height,
                },
                Some(frame.pixels.as_ptr().cast()),
                frame.width * 4,
                &properties,
            )?
        });
        Ok(())
    }

    fn image_area(&self) -> RECT {
        let image = self.chrome_layout().image;
        RECT {
            left: image.left.round() as i32,
            top: image.top.round() as i32,
            right: image.right.round() as i32,
            bottom: image.bottom.round() as i32,
        }
    }

    fn calculate_view(&mut self) -> Option<D2D_RECT_F> {
        let frame = self.document.as_ref()?.current_frame();
        let area = self.image_area();
        let aw = (area.right - area.left).max(1) as f32;
        let ah = (area.bottom - area.top).max(1) as f32;
        let iw = frame.width as f32;
        let ih = frame.height as f32;
        self.zoom = match self.zoom_mode {
            ZoomMode::Fit => (aw / iw).min(ah / ih),
            ZoomMode::FitWidth => (aw / iw).max(0.001),
            ZoomMode::Actual => 1.0,
            ZoomMode::Custom => self.zoom.clamp(0.01, 64.0),
        };
        if !matches!(self.zoom_mode, ZoomMode::Custom) {
            self.center = (iw / 2.0, ih / 2.0);
        }
        let cx = (area.left + area.right) as f32 / 2.0;
        let cy = (area.top + area.bottom) as f32 / 2.0;
        Some(D2D_RECT_F {
            left: cx - self.center.0 * self.zoom,
            top: cy - self.center.1 * self.zoom,
            right: cx + (iw - self.center.0) * self.zoom,
            bottom: cy + (ih - self.center.1) * self.zoom,
        })
    }

    unsafe fn paint(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
        if unsafe { self.ensure_target() }.is_err() {
            return;
        }
        if let Err(error) = unsafe { self.ensure_bitmap() } {
            eprintln!("KGen View Direct2D bitmap error: {error:?}");
        }
        let palette = Palette::for_dark(self.settings.is_dark());
        let target = self.target.as_ref().unwrap().clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&palette.canvas));
        }
        let destination = self.calculate_view();
        if let (Some(bitmap), Some(destination)) = (self.bitmap.as_ref(), destination) {
            unsafe {
                target.DrawBitmap(
                    bitmap,
                    Some(&destination),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
        }
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let rect = D2D_RECT_F {
                left: start.x.min(end.x) as f32,
                top: start.y.min(end.y) as f32,
                right: start.x.max(end.x) as f32,
                bottom: start.y.max(end.y) as f32,
            };
            let color = D2D1_COLOR_F {
                r: 1.0,
                g: 0.54,
                b: 0.16,
                a: 1.0,
            };
            if let Ok(brush) = unsafe { target.CreateSolidColorBrush(&color, None) } {
                unsafe { target.DrawRectangle(&rect, &brush, 2.0, None) };
            }
        }
        let layout = self.chrome_layout();
        let mut status = self.status_model.clone();
        if let Some(label) = self.chrome.label_for_hover() {
            status.primary = label.to_string();
        }
        let active_command = match self.zoom_mode {
            ZoomMode::Fit => Some(cmd::FIT),
            ZoomMode::FitWidth => Some(cmd::FIT_WIDTH),
            ZoomMode::Actual => Some(cmd::ACTUAL),
            ZoomMode::Custom => None,
        };
        unsafe {
            self.chrome.draw(
                &target,
                &layout,
                palette,
                active_command,
                self.lang(),
                self.settings.is_dark(),
            );
        }
        if unsafe { target.EndDraw(None, None) }.is_err() {
            self.target = None;
            self.bitmap = None;
            self.chrome.reset_render_resources();
        } else {
            unsafe {
                crate::ui::status::draw(hdc, &layout, &status, self.settings.is_dark());
            }
        }
    }

    fn invalidate_image(&mut self) {
        self.bitmap = None;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.update_status(None);
    }

    fn update_status(&mut self, temporary: Option<&str>) {
        self.status_model = if let Some(value) = temporary {
            StatusModel::message(value)
        } else if let Some(document) = &self.document {
            let frame = document.current_frame();
            let folder = self
                .catalog
                .index
                .map(|i| format!("{}/{}", i + 1, self.catalog.files.len()))
                .unwrap_or_else(|| "—".into());
            let disk_size = crate::platform::text::format_bytes_decimal(document.file_size);
            let ram_size = crate::platform::text::format_bytes_decimal(frame.pixels.len() as u64);
            let file_time = file_timestamp_short(&document.path);
            StatusModel {
                primary: format!("{} x {} x 24 BPP", frame.width, frame.height),
                position: Some(folder),
                zoom: Some(format!("{:.0} %", self.zoom * 100.0)),
                size: Some(format!("{} / {}", disk_size, ram_size)),
                date: Some(file_time),
                message: None,
            }
        } else {
            StatusModel::message(tr(self.lang(), Text::NoImage))
        };
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn start_load(&mut self, path: PathBuf) {
        // Catalog entries are already absolute after the first load. Avoid a
        // filesystem canonicalization on every wheel event; normalize only
        // the initial load and external relative paths.
        let path = if path.is_absolute() && !self.catalog.files.is_empty() {
            path
        } else {
            path.canonicalize().unwrap_or(path)
        };
        if !path.is_file() {
            return;
        }
        self.active_path = Some(path.clone());
        self.generation = self.generation.wrapping_add(1);
        self.load_started = Some(Instant::now());
        let generation = self.generation;

        if let Some(index) = self.cache.iter().position(|(cached, _)| *cached == path) {
            let (_, document) = self.cache.remove(index);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&document));
            self.load_time_ms = 0;
            self.load_started = None;
            let mut catalog = self.catalog.clone();
            let is_same_folder = catalog.files.first().and_then(|p| p.parent()) == path.parent();
            if catalog.files.is_empty() || !is_same_folder {
                catalog = FolderCatalog::default();
            } else {
                catalog.update_index(&path);
            }
            unsafe {
                self.accept_load(LoadPayload {
                    generation,
                    path,
                    result: Ok(document),
                    catalog,
                });
            }
            return;
        }

        self.update_status(Some(tr(self.lang(), Text::Loading)));
        self.loader.submit(generation, path, self.hwnd);
    }

    unsafe fn accept_load(&mut self, payload: LoadPayload) {
        if payload.generation != self.generation {
            return;
        }
        match payload.result {
            Ok(document) => {
                if self.load_started.is_some() {
                    self.load_time_ms = self
                        .load_started
                        .take()
                        .map(|started| started.elapsed().as_millis())
                        .unwrap_or(0);
                }
                let is_same_folder =
                    self.catalog.files.first().and_then(|p| p.parent()) == payload.path.parent();
                let needs_catalog = payload.catalog.files.is_empty() && !is_same_folder;
                if payload.catalog.files.is_empty() || is_same_folder {
                    if is_same_folder {
                        self.catalog.update_index(&payload.path);
                    } else {
                        self.catalog = FolderCatalog::default();
                        self.catalog_stamp = None;
                    }
                } else {
                    self.catalog = payload.catalog;
                    self.catalog_stamp = folder_modified(&payload.path);
                }
                self.failed_paths.remove(&payload.path);
                let frame = document.current_frame();
                self.center = (frame.width as f32 / 2.0, frame.height as f32 / 2.0);
                self.document = Some(document);
                self.zoom_mode = ZoomMode::Fit;
                self.crop_selection = None;
                self.bitmap = None;
                self.set_animation_timer();
                self.update_title();
                if self.settings.auto_resize && !self.fullscreen {
                    self.schedule_auto_resize();
                }
                self.invalidate_image();
                if needs_catalog {
                    self.catalog_loader
                        .submit(self.generation, payload.path.clone(), self.hwnd);
                } else if self.prefetch_enabled {
                    self.start_prefetch();
                }
            }
            Err(error) => {
                let is_same_folder =
                    self.catalog.files.first().and_then(|p| p.parent()) == payload.path.parent();
                if payload.catalog.files.is_empty() && is_same_folder {
                    self.catalog.update_index(&payload.path);
                } else {
                    self.catalog = payload.catalog;
                    self.catalog_stamp = folder_modified(&payload.path);
                }
                self.failed_paths.insert(payload.path.clone());
                let extension = payload
                    .path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let suffix = if matches!(extension.as_str(), "webp" | "heic" | "heif") {
                    format!("\n\n{}", tr(self.lang(), Text::CodecMissing))
                } else {
                    String::new()
                };
                let next = (1..=self.catalog.files.len()).find_map(|offset| {
                    self.catalog
                        .adjacent_wrapped(offset as isize)
                        .filter(|candidate| !self.failed_paths.contains(candidate))
                });
                if let Some(next) = next {
                    self.update_status(Some(if self.lang() == Language::Spanish {
                        "Imagen dañada o incompatible; pasando a la siguiente…"
                    } else {
                        "Damaged or unsupported image; skipping to the next…"
                    }));
                    self.start_load(next);
                } else {
                    self.update_status(Some(tr(self.lang(), Text::Unsupported)));
                    self.show_error(&format!("{}{}", error, suffix));
                }
            }
        }
    }

    fn start_prefetch(&self) {
        // Do not spend CPU preloading while the viewer is simply idle on its
        // first image. Once the user starts navigating, warm only the next
        // image in the current direction.
        let offset = if self.navigation_direction < 0 { -1 } else { 1 };
        let offsets = [offset];
        let candidates: Vec<PathBuf> = offsets
            .into_iter()
            .filter_map(|offset| self.catalog.adjacent_wrapped(offset))
            .filter(|path| !self.cache.iter().any(|(cached, _)| cached == path))
            .collect();
        self.loader
            .submit_prefetch(self.generation, candidates, self.hwnd);
    }

    fn accept_prefetch(&mut self, payload: PrefetchPayload) {
        if payload.generation != self.generation
            || self.cache.iter().any(|(path, _)| *path == payload.path)
        {
            return;
        }
        let Ok(document) = payload.result else { return };
        let bytes = document_bytes(&document);
        if bytes > 32 * 1024 * 1024 {
            return;
        }
        let max_cache_bytes = 64 * 1024 * 1024;
        while self.cache_bytes + bytes > max_cache_bytes {
            if self.cache.is_empty() {
                break;
            }
            let evicted = self.cache.remove(0);
            self.cache_bytes = self.cache_bytes.saturating_sub(document_bytes(&evicted.1));
            drop(evicted);
        }
        if self.cache_bytes + bytes > max_cache_bytes {
            return;
        }
        self.cache.push((payload.path, document));
        self.cache_bytes += bytes;
    }

    fn accept_catalog(&mut self, payload: CatalogPayload) {
        if payload.generation != self.generation
            || self.load_started.is_some()
            || self
                .document
                .as_ref()
                .is_none_or(|document| document.path != payload.path)
        {
            return;
        }
        self.catalog = payload.catalog;
        self.catalog_stamp = folder_modified(&payload.path);
        self.update_status(None);
        if self.prefetch_enabled {
            self.start_prefetch();
        }
    }

    fn schedule_auto_resize(&self) {
        if self.fullscreen {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_AUTO_RESIZE);
            let _ = SetTimer(
                Some(self.hwnd),
                TIMER_AUTO_RESIZE,
                AUTO_RESIZE_DELAY_MS,
                None,
            );
        }
    }

    unsafe fn auto_resize_window(&mut self) {
        if self.fullscreen {
            return;
        }
        let Some(document) = &self.document else {
            return;
        };
        let frame = document.current_frame();
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return;
        }
        let max_w = info.rcWork.right - info.rcWork.left;
        let max_h = info.rcWork.bottom - info.rcWork.top;
        let chrome = self.chrome_layout();
        let mut frame_probe = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let _ = unsafe {
            AdjustWindowRectEx(
                &mut frame_probe,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            )
        };
        let frame_w = frame_probe.right - frame_probe.left - 100;
        let frame_h = frame_probe.bottom - frame_probe.top - 100;
        let chrome_h = chrome.toolbar_height() + chrome.status_height();
        let max_image_w = (max_w - frame_w).max(1) as f32;
        let max_image_h = (max_h - frame_h - chrome_h).max(1) as f32;
        let scale = (max_image_w / frame.width.max(1) as f32)
            .min(max_image_h / frame.height.max(1) as f32)
            .min(1.0);
        let desired_client_w = (frame.width as f32 * scale).round() as i32;
        let desired_client_h = (frame.height as f32 * scale).round() as i32 + chrome_h;
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: desired_client_w.max(220),
            bottom: desired_client_h.max(160),
        };
        let _ = unsafe {
            AdjustWindowRectEx(
                &mut outer,
                WS_OVERLAPPEDWINDOW,
                false,
                WINDOW_EX_STYLE::default(),
            )
        };
        let width = (outer.right - outer.left).min(max_w).max(220);
        let height = (outer.bottom - outer.top).min(max_h).max(150);
        let mut current = RECT::default();
        let _ = unsafe { GetWindowRect(self.hwnd, &mut current) };
        let x = current.left.clamp(
            info.rcWork.left,
            (info.rcWork.right - width).max(info.rcWork.left),
        );
        let y = current.top.clamp(
            info.rcWork.top,
            (info.rcWork.bottom - height).max(info.rcWork.top),
        );
        if current.left == x
            && current.top == y
            && current.right - current.left == width
            && current.bottom - current.top == height
        {
            return;
        }
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }

    fn update_title(&self) {
        let title = self
            .document
            .as_ref()
            .and_then(|doc| doc.path.file_name())
            .map(|name| format!("{} — KGen View", name.to_string_lossy()))
            .unwrap_or_else(|| "KGen View".into());
        let wide = wide(&title);
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
        }
    }

    fn navigate(&mut self, offset: isize) {
        let Some(path) = self.catalog.adjacent_wrapped(offset) else {
            return;
        };
        if self.can_leave() {
            // Advance the logical position immediately. A fast wheel gesture
            // can then skip over images still being decoded and the worker
            // only has to finish the newest destination.
            self.prefetch_enabled = true;
            self.navigation_direction = offset.signum();
            self.catalog.update_index(&path);
            self.start_load(path);
        }
    }

    fn navigate_edge(&mut self, first: bool) {
        let path = if first {
            self.catalog.first()
        } else {
            self.catalog.last()
        };
        if let Some(path) = path
            && self.can_leave()
        {
            self.prefetch_enabled = true;
            self.navigation_direction = if first { 1 } else { -1 };
            self.catalog.update_index(&path);
            self.start_load(path);
        }
    }

    fn can_leave(&mut self) -> bool {
        if !self.document.as_ref().is_some_and(|doc| doc.dirty) {
            return true;
        }
        let caption = wide(tr(self.lang(), Text::AppTitle));
        let text = wide(tr(self.lang(), Text::SaveChanges));
        let answer = unsafe {
            MessageBoxW(
                Some(self.hwnd),
                PCWSTR(text.as_ptr()),
                PCWSTR(caption.as_ptr()),
                MB_YESNOCANCEL | MB_ICONQUESTION,
            )
        };
        match answer.0 {
            6 => self.save_as_dialog(),
            7 => true,
            _ => false,
        }
    }

    fn open_dialog(&mut self) {
        let mut buffer = vec![0u16; 32768];
        let filter = wide_with_nuls(
            "Imágenes compatibles\0*.bmp;*.dib;*.jpg;*.jpeg;*.jpe;*.png;*.gif;*.tif;*.tiff;*.ico;*.webp;*.heic;*.heif;*.jxr;*.wdp;*.dds\0Todos los archivos\0*.*\0\0",
        );
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: self.hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY,
            ..Default::default()
        };
        if unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..nul_pos(&buffer)]));
            if self.can_leave() {
                self.start_load(path);
            }
        }
    }

    fn save_as_dialog(&mut self) -> bool {
        let Some(document) = &self.document else {
            return false;
        };
        let mut buffer = vec![0u16; 32768];
        let suggested = document
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let suggested_w = wide(&suggested);
        let count = suggested_w.len().min(buffer.len());
        buffer[..count].copy_from_slice(&suggested_w[..count]);
        let filter =
            wide_with_nuls("PNG\0*.png\0JPEG\0*.jpg;*.jpeg\0Bitmap\0*.bmp\0TIFF\0*.tif;*.tiff\0\0");
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: self.hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT,
            lpstrDefExt: w!("png"),
            ..Default::default()
        };
        if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
            return false;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..nul_pos(&buffer)]));
        let result = self.document.as_mut().unwrap().save_as(&path);
        match result {
            Ok(()) => {
                self.catalog = FolderCatalog::scan(&path);
                self.update_title();
                self.update_status(None);
                true
            }
            Err(error) => {
                self.show_error(&error.to_string());
                false
            }
        }
    }

    fn show_error(&self, text: &str) {
        let text = wide(text);
        let caption = wide(tr(self.lang(), Text::Error));
        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                PCWSTR(text.as_ptr()),
                PCWSTR(caption.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn info(&mut self) {
        let Some(doc) = &self.document else { return };
        let frame = doc.current_frame();
        let pixels = u64::from(frame.width) * u64::from(frame.height);
        let unique_colors = if pixels <= 4_000_000 {
            let count = frame
                .pixels
                .chunks_exact(4)
                .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
                .collect::<HashSet<_>>()
                .len();
            count.to_string()
        } else {
            "Más de 4 millones (imagen grande)".into()
        };
        let author = metadata_value(doc, "Autor");
        let description = metadata_value(doc, "Descripción");
        let folder = doc.path.parent().map(display_path).unwrap_or_default();
        let file_name = doc
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let position = self
            .catalog
            .index
            .map(|index| format!("{} / {}", index + 1, self.catalog.files.len()))
            .unwrap_or_else(|| "—".into());
        let ratio = frame.width as f64 / frame.height.max(1) as f64;
        let data = PropertiesData {
            file_name,
            folder,
            full_path: display_path(&doc.path),
            format: format!("{} · WIC/GIF nativo", doc.format_name),
            resolution: "96 × 96 DPI".into(),
            original_size: format!("{} × {} píxeles ({ratio:.2})", frame.width, frame.height),
            current_size: format!("{} × {} píxeles ({ratio:.2})", frame.width, frame.height),
            print_size: format!(
                "{:.2} × {:.2} cm · {:.2} × {:.2} pulgadas",
                frame.width as f64 / 96.0 * 2.54,
                frame.height as f64 / 96.0 * 2.54,
                frame.width as f64 / 96.0,
                frame.height as f64 / 96.0,
            ),
            original_colors: "16,7 millones · 32 bits BGRA".into(),
            current_colors: "16,7 millones · 32 bits BGRA".into(),
            unique_colors,
            disk_size: format!("{} ({} bytes)", format_bytes(doc.file_size), doc.file_size),
            memory_size: format_bytes(frame.pixels.len() as u64),
            folder_index: position,
            file_time: file_timestamp(&doc.path),
            load_time: format!("{} milisegundos", self.load_time_ms),
            author,
            description,
            api_key: self.settings.imgbb_api_key.clone(),
        };
        let Some(edit) =
            crate::platform::properties_dialog::show(self.hwnd, self.settings.is_dark(), data)
        else {
            return;
        };
        self.apply_properties(edit);
    }

    fn apply_properties(&mut self, edit: PropertiesEdit) {
        if self.settings.imgbb_api_key != edit.api_key.trim() {
            self.settings.imgbb_api_key = edit.api_key.trim().into();
            self.settings.save();
        }
        let Some(document) = &mut self.document else {
            return;
        };
        let old_path = document.path.clone();
        let old_name = old_path.file_name().unwrap_or_default().to_string_lossy();
        let new_name = edit.file_name.trim();
        if !new_name.is_empty() && new_name != old_name {
            if new_name.contains(['/', '\\']) {
                self.show_error("El nombre no puede contener separadores de carpeta.");
                return;
            }
            let target = old_path.with_file_name(new_name);
            if target.exists() {
                self.show_error("Ya existe un archivo con ese nombre.");
                return;
            }
            if let Err(error) = fs::rename(&old_path, &target) {
                self.show_error(&format!("No se pudo renombrar el archivo: {error}"));
                return;
            }
            document.path = target.clone();
            self.active_path = Some(target.clone());
            self.catalog = FolderCatalog::scan(&target);
        }
        let changed = set_metadata_value(document, "Autor", edit.author.trim())
            | set_metadata_value(document, "Descripción", edit.description.trim());
        if changed {
            document.dirty = true;
        }
        self.update_title();
        self.update_status(None);
    }

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

    fn delete_current(&mut self) {
        let Some(doc) = &self.document else { return };
        if self.settings.confirm_delete {
            let text = wide(tr(self.lang(), Text::ConfirmDelete));
            let caption = wide(tr(self.lang(), Text::AppTitle));
            if unsafe {
                MessageBoxW(
                    Some(self.hwnd),
                    PCWSTR(text.as_ptr()),
                    PCWSTR(caption.as_ptr()),
                    MB_YESNO | MB_ICONQUESTION,
                )
            }
            .0 != 6
            {
                return;
            }
        }
        let path = doc.path.clone();
        let next = self
            .catalog
            .adjacent(1)
            .or_else(|| self.catalog.adjacent(-1));
        let mut from = wide_path(&path);
        from.push(0);
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: self.hwnd,
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO.0 | FOF_NOERRORUI.0) as u16,
            ..Default::default()
        };
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result == 0 && !operation.fAnyOperationsAborted.as_bool() {
            self.document = None;
            self.bitmap = None;
            if let Some(next) = next {
                self.start_load(next);
            } else {
                self.catalog = FolderCatalog::default();
                self.update_title();
                self.invalidate_image();
            }
        }
    }

    fn copy_path(&self) {
        if let Some(doc) = &self.document {
            copy_text(self.hwnd, &doc.path.to_string_lossy());
        }
    }

    fn copy_image(&self) {
        let Some(doc) = &self.document else { return };
        let frame = doc.current_frame();
        copy_bgra_image(self.hwnd, frame.width, frame.height, &frame.pixels);
    }

    fn copy_url(&mut self) {
        if self.uploading {
            self.update_status(Some("La imagen ya se está subiendo a ImgBB…"));
            return;
        }
        if self.settings.imgbb_api_key.trim().is_empty() {
            self.info();
            if self.settings.imgbb_api_key.trim().is_empty() {
                self.show_error(
                    "Introduce tu clave API de ImgBB en Información y pulsa Guardar. La clave se obtiene gratuitamente en api.imgbb.com.",
                );
                return;
            }
        }
        let Some(document) = &self.document else {
            return;
        };
        let path = document.path.clone();
        let api_key = self.settings.imgbb_api_key.clone();
        let alive = self.alive.clone();
        let hwnd_raw = self.hwnd.0 as usize;
        self.uploading = true;
        self.update_status(Some("Subiendo a ImgBB…"));
        thread::spawn(move || {
            let result = imgbb::upload(&path, &api_key);
            if !alive.load(Ordering::Acquire) {
                return;
            }
            let payload = Box::new(UploadPayload { result });
            let raw = Box::into_raw(payload);
            let hwnd = HWND(hwnd_raw as *mut c_void);
            if unsafe { PostMessageW(Some(hwnd), WM_UPLOAD_DONE, WPARAM(0), LPARAM(raw as isize)) }
                .is_err()
            {
                unsafe {
                    drop(Box::from_raw(raw));
                }
            }
        });
    }

    fn accept_upload(&mut self, payload: UploadPayload) {
        self.uploading = false;
        match payload.result {
            Ok(url) => {
                copy_text(self.hwnd, &url);
                self.update_status(Some("Enlace directo copiado al portapapeles"));
            }
            Err(error) => {
                self.update_status(None);
                self.show_error(&error);
            }
        }
    }

    fn open_folder(&self) {
        let Some(doc) = &self.document else { return };
        let argument = format!("/select,\"{}\"", doc.path.display());
        let argument = wide(&argument);
        unsafe {
            ShellExecuteW(
                Some(self.hwnd),
                w!("open"),
                w!("explorer.exe"),
                PCWSTR(argument.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    unsafe fn rebuild_menu(&mut self) {
        if let Ok(menu) = ThemedMenu::new(self.hwnd, self.lang(), self.settings.is_dark()) {
            self.menu = Some(menu);
            let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
        }
    }

    fn show_menu(&self, index: usize, layout: &ChromeLayout) {
        let Some(item) = layout.menu_items.iter().find(|item| item.index == index) else {
            return;
        };
        let mut anchor = POINT {
            x: item.rect.left.round() as i32,
            y: item.rect.bottom.round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut anchor);
            if let Some(menu) = &self.menu {
                if index == 6 {
                    menu.show_all(self.hwnd, anchor.x, anchor.y);
                } else {
                    menu.show_category(self.hwnd, index, anchor.x, anchor.y);
                }
            }
        }
    }

    fn handle_command(&mut self, id: usize) {
        match id {
            cmd::OPEN => self.open_dialog(),
            cmd::SAVE_AS => {
                self.save_as_dialog();
            }
            cmd::EXIT => unsafe {
                let _ = SendMessageW(self.hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
            },
            cmd::UNDO => self.edit(|doc| {
                doc.undo();
            }),
            cmd::REDO => self.edit(|doc| {
                doc.redo();
            }),
            cmd::COPY => self.copy_image(),
            cmd::COPY_URL => self.copy_url(),
            cmd::CROP => self.crop(),
            cmd::DELETE => self.delete_current(),
            cmd::ROTATE_LEFT => self.edit(ImageDocument::rotate_left),
            cmd::ROTATE_RIGHT => self.edit(ImageDocument::rotate_right),
            cmd::FLIP_H => self.edit(ImageDocument::flip_horizontal),
            cmd::FLIP_V => self.edit(ImageDocument::flip_vertical),
            cmd::PREVIOUS => self.navigate(-1),
            cmd::NEXT => self.navigate(1),
            cmd::ACTUAL => self.set_zoom(ZoomMode::Actual),
            cmd::FIT => self.set_zoom(ZoomMode::Fit),
            cmd::FIT_WIDTH => self.set_zoom(ZoomMode::FitWidth),
            cmd::FULLSCREEN => self.toggle_fullscreen(),
            cmd::SLIDESHOW => self.toggle_slideshow(),
            cmd::PAUSE_ANIMATION => {
                self.animation_playing = !self.animation_playing;
                self.set_animation_timer();
                self.update_status(None);
            }
            cmd::RELOAD => {
                if let Some(doc) = &self.document {
                    let path = doc.path.clone();
                    if self.can_leave() {
                        self.start_load(path);
                    }
                }
            }
            cmd::INFO => self.info(),
            cmd::OPEN_FOLDER => self.open_folder(),
            cmd::COPY_PATH => self.copy_path(),
            cmd::PREVIOUS_FRAME => {
                if let Some(doc) = &mut self.document {
                    doc.previous_frame();
                    self.invalidate_image();
                }
            }
            cmd::NEXT_FRAME => {
                if let Some(doc) = &mut self.document {
                    doc.next_frame();
                    self.invalidate_image();
                }
            }
            cmd::LANGUAGE => {
                self.settings.language = self.settings.language.toggle();
                self.settings.save();
                unsafe {
                    self.rebuild_menu();
                }
                self.update_title();
                self.update_status(None);
            }
            cmd::THEME => {
                self.settings.cycle_theme();
                unsafe {
                    self.apply_theme();
                    self.rebuild_menu();
                }
            }
            cmd::ABOUT => {
                let text = wide(
                    "KGen View 1.2.0\n\nVisor nativo para Windows\nRust + WIC + Direct2D\nLicencia MIT\n\nSin telemetría. La red solo se usa al pulsar Copiar enlace (ImgBB).",
                );
                let caption = wide("KGen View");
                unsafe {
                    MessageBoxW(
                        Some(self.hwnd),
                        PCWSTR(text.as_ptr()),
                        PCWSTR(caption.as_ptr()),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
            }
            _ => {}
        }
    }
}

pub fn run() -> Result<(), WinError> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    if let Some(path) = command_line_path()
        && send_to_existing(&path)
    {
        unsafe { CoUninitialize() };
        return Ok(());
    }
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    // Win32 encodes integer resource identifiers in the low word of a pointer.
    #[allow(clippy::manual_dangling_ptr)]
    let icon = unsafe {
        LoadIconW(Some(instance), PCWSTR(1usize as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))?
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: icon,
        hIconSm: icon,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(WinError::empty());
    }
    let settings = Settings::load();
    let raw_state = Box::into_raw(Box::new(AppState::new(settings)?));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("KGen View"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            620,
            None,
            None,
            Some(instance),
            Some(raw_state.cast()),
        )
    };
    let hwnd = match hwnd {
        Ok(value) => value,
        Err(error) => {
            unsafe {
                drop(Box::from_raw(raw_state));
            }
            return Err(error);
        }
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }
    if let Some(path) = command_line_path() {
        unsafe {
            state_mut(hwnd).start_load(path);
        }
    }
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { CoUninitialize() };
    Ok(())
}

pub fn fatal_error(message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("KGen View"),
            MB_OK | MB_ICONERROR,
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        let state = unsafe { (*create).lpCreateParams as *mut AppState };
        if !state.is_null() {
            unsafe {
                (*state).hwnd = hwnd;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            }
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState };
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_CREATE => {
            unsafe {
                state.create_controls();
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(target) = &state.target {
                let width = loword(lparam.0 as usize) as u32;
                let height = hiword(lparam.0 as usize) as u32;
                if width > 0
                    && height > 0
                    && unsafe { target.Resize(&D2D_SIZE_U { width, height }) }.is_err()
                {
                    state.target = None;
                    state.bitmap = None;
                    state.chrome.reset_render_resources();
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                state.rebuild_menu();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if state.settings.theme == ThemeMode::System {
                unsafe {
                    state.apply_theme();
                    state.rebuild_menu();
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
                state.paint(ps.hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MEASUREITEM => {
            let handled = state.menu.as_ref().is_some_and(|menu| unsafe {
                menu.measure(lparam.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT)
            });
            if handled {
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_DRAWITEM => {
            let handled = state.menu.as_ref().is_some_and(|menu| unsafe {
                menu.draw(lparam.0 as *mut windows::Win32::UI::Controls::DRAWITEMSTRUCT)
            });
            if handled {
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            state.handle_command(loword(wparam.0) as usize);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            handle_key(state, wparam.0 as u32);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(wparam.0) as i16;
            if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
                state.change_zoom(if delta > 0 { 1.15 } else { 1.0 / 1.15 });
            } else {
                state.navigate(if delta > 0 { -1 } else { 1 });
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            if state.chrome.menu.press(&layout, point).is_some()
                || state.chrome.toolbar.press(&layout, point).is_some()
            {
                unsafe {
                    SetCapture(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            } else if point.y >= state.image_area().top && point.y < state.image_area().bottom {
                state.selection_start = Some(point);
                state.selection_end = Some(point);
                unsafe {
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            let menu_changed = state.chrome.menu.update_hover(&layout, point);
            let toolbar_changed = state.chrome.toolbar.update_hover(&layout, point);
            if menu_changed || toolbar_changed {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            if state.selection_start.is_some() {
                state.selection_end = Some(point);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            if wparam.0 & 0x0010 != 0
                && let Some(previous) = state.middle_start.replace(point)
                && state.document.is_some()
            {
                state.zoom_mode = ZoomMode::Custom;
                state.center.0 -= (point.x - previous.x) as f32 / state.zoom;
                state.center.1 -= (point.y - previous.y) as f32 / state.zoom;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let point = point_from_lparam(lparam);
            let layout = state.chrome_layout();
            let menu_interaction = state.chrome.menu.pressed.is_some();
            let toolbar_interaction = state.chrome.toolbar.pressed.is_some();
            let menu_index = state.chrome.menu.release(&layout, point);
            let command = state.chrome.toolbar.release(&layout, point);
            unsafe {
                let _ = ReleaseCapture();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            if menu_interaction {
                if let Some(index) = menu_index {
                    state.show_menu(index, &layout);
                }
            } else if toolbar_interaction {
                if let Some(command) = command {
                    state.handle_command(command);
                }
            } else {
                state.finish_selection();
            }
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            state.middle_start = Some(point_from_lparam(lparam));
            unsafe {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            state.middle_start = None;
            unsafe {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_DROPFILES => {
            let drop = windows::Win32::UI::Shell::HDROP(wparam.0 as *mut c_void);
            let length = unsafe { DragQueryFileW(drop, 0, None) };
            if length > 0 {
                let mut buffer = vec![0u16; length as usize + 1];
                unsafe {
                    DragQueryFileW(drop, 0, Some(&mut buffer));
                    DragFinish(drop);
                }
                let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
                if state.can_leave() {
                    state.start_load(path);
                }
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_COPYDATA => {
            let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            if data.dwData != IMAGE_HANDOFF_DATA || data.cbData < 2 || data.lpData.is_null() {
                return LRESULT(0);
            }
            let slice = unsafe {
                std::slice::from_raw_parts(data.lpData.cast::<u16>(), data.cbData as usize / 2)
            };
            let end = slice
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(slice.len());
            let incoming = PathBuf::from(String::from_utf16_lossy(&slice[..end]));
            if state
                .active_path
                .as_deref()
                .is_some_and(|active| paths_equal(active, &incoming))
            {
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_LOAD_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut LoadPayload) };
            unsafe {
                state.accept_load(*payload);
            }
            LRESULT(0)
        }
        WM_PREFETCH_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut PrefetchPayload) };
            state.accept_prefetch(*payload);
            LRESULT(0)
        }
        WM_CATALOG_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut CatalogPayload) };
            state.accept_catalog(*payload);
            LRESULT(0)
        }
        WM_UPLOAD_DONE => {
            let payload = unsafe { Box::from_raw(lparam.0 as *mut UploadPayload) };
            state.accept_upload(*payload);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATION {
                if let Some(doc) = &mut state.document {
                    doc.next_frame();
                    state.bitmap = None;
                    state.invalidate_image();
                    state.set_animation_timer();
                }
            } else if wparam.0 == TIMER_SLIDESHOW {
                state.navigate(1);
            } else if wparam.0 == TIMER_FOLDER_REFRESH
                && let Some(document) = &state.document
            {
                if folder_modified(&document.path) != state.catalog_stamp {
                    state.catalog_loader.submit(
                        state.generation,
                        document.path.clone(),
                        state.hwnd,
                    );
                }
            } else if wparam.0 == TIMER_AUTO_RESIZE {
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_AUTO_RESIZE);
                    if state.settings.auto_resize && !state.fullscreen {
                        state.auto_resize_window();
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if state.can_leave() {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            state.alive.store(false, Ordering::Release);
            state.settings.save();
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn handle_key(state: &mut AppState, key: u32) {
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match key {
        k if k == VK_LEFT.0 as u32 => state.navigate(-1),
        k if k == VK_RIGHT.0 as u32 => state.navigate(1),
        k if k == VK_HOME.0 as u32 => state.navigate_edge(true),
        k if k == VK_END.0 as u32 => state.navigate_edge(false),
        k if k == VK_DELETE.0 as u32 => state.delete_current(),
        k if k == VK_F11.0 as u32 => state.toggle_fullscreen(),
        k if k == VK_F5.0 as u32 => state.toggle_slideshow(),
        k if k == VK_F9.0 as u32 => state.handle_command(cmd::RELOAD),
        k if k == VK_SPACE.0 as u32 => state.handle_command(cmd::PAUSE_ANIMATION),
        k if k == VK_PRIOR.0 as u32 => state.handle_command(cmd::PREVIOUS_FRAME),
        k if k == VK_NEXT.0 as u32 => state.handle_command(cmd::NEXT_FRAME),
        k if k == VK_ESCAPE.0 as u32 => {
            if state.fullscreen {
                state.toggle_fullscreen();
            }
        }
        k if k == VK_ADD.0 as u32 || k == VK_OEM_PLUS.0 as u32 => state.change_zoom(1.15),
        k if k == VK_SUBTRACT.0 as u32 || k == VK_OEM_MINUS.0 as u32 => {
            state.change_zoom(1.0 / 1.15)
        }
        0x31 if !ctrl => state.set_zoom(ZoomMode::Actual),
        0x46 if !ctrl => state.set_zoom(ZoomMode::Fit),
        0x49 if !ctrl => state.info(),
        0x4F if ctrl => state.open_dialog(),
        0x53 if ctrl && shift => {
            state.save_as_dialog();
        }
        0x43 if ctrl => state.copy_image(),
        0x58 if ctrl && shift => state.crop(),
        0x5A if ctrl => state.handle_command(cmd::UNDO),
        0x59 if ctrl => state.handle_command(cmd::REDO),
        _ => {}
    }
}

fn metadata_value(document: &ImageDocument, label: &str) -> String {
    document
        .metadata
        .iter()
        .find(|(candidate, _)| candidate == label)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn document_bytes(document: &ImageDocument) -> usize {
    document.frames.iter().map(|frame| frame.pixels.len()).sum()
}

fn set_metadata_value(document: &mut ImageDocument, label: &str, value: &str) -> bool {
    if let Some(index) = document
        .metadata
        .iter()
        .position(|(candidate, _)| candidate == label)
    {
        if document.metadata[index].1 == value {
            return false;
        }
        if value.is_empty() {
            document.metadata.remove(index);
        } else {
            document.metadata[index].1 = value.into();
        }
        true
    } else if value.is_empty() {
        false
    } else {
        document.metadata.push((label.into(), value.into()));
        true
    }
}

fn file_timestamp(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return "—".into();
    };
    let raw = metadata.last_write_time();
    let file_time = FILETIME {
        dwLowDateTime: raw as u32,
        dwHighDateTime: (raw >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.is_err()
        || unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err()
    {
        return "—".into();
    }
    format!(
        "{:02}/{:02}/{:04} · {:02}:{:02}:{:02}",
        local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute, local.wSecond
    )
}

fn file_timestamp_short(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return "—".into();
    };
    let raw = metadata.last_write_time();
    let file_time = FILETIME {
        dwLowDateTime: raw as u32,
        dwHighDateTime: (raw >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file_time, &mut utc) }.is_err()
        || unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err()
    {
        return "—".into();
    }
    format!(
        "{:02}/{:02}/{:04} / {:02}:{:02}:{:02}",
        local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute, local.wSecond
    )
}

fn folder_modified(path: &Path) -> Option<SystemTime> {
    path.parent().and_then(|folder| {
        fs::metadata(folder)
            .and_then(|metadata| metadata.modified())
            .ok()
    })
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        value.into_owned()
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn command_line_path() -> Option<PathBuf> {
    std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn send_to_existing(path: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let wide = wide_path(&path);
    let data = COPYDATASTRUCT {
        dwData: IMAGE_HANDOFF_DATA,
        cbData: (wide.len() * 2) as u32,
        lpData: wide.as_ptr() as *mut c_void,
    };
    let mut previous = None;
    loop {
        let Ok(hwnd) = (unsafe { FindWindowExW(None, previous, CLASS_NAME, None) }) else {
            break;
        };
        let matched = unsafe {
            SendMessageW(
                hwnd,
                WM_COPYDATA,
                Some(WPARAM(0)),
                Some(LPARAM((&data as *const COPYDATASTRUCT) as isize)),
            )
        };
        if matched.0 == 1 {
            return true;
        }
        previous = Some(hwnd);
    }
    false
}

unsafe fn state_mut<'a>(hwnd: HWND) -> &'a mut AppState {
    unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState) }
}

fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}
fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}
fn point_from_lparam(value: LPARAM) -> POINT {
    POINT {
        x: loword(value.0 as usize) as i16 as i32,
        y: hiword(value.0 as usize) as i16 as i32,
    }
}
