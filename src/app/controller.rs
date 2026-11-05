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
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
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
                ScreenToClient, UpdateWindow,
            },
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
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
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_STYLE,
                GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowRect,
                HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IDI_APPLICATION, IsIconic, IsWindow,
                IsZoomed, KillTimer, LoadCursorW, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION,
                MB_ICONQUESTION, MB_OK, MB_YESNO, MB_YESNOCANCEL, MSG, MessageBoxW, PostMessageW,
                PostQuitMessage, RegisterClassExW, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
                WM_DRAWITEM, WM_DROPFILES, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MEASUREITEM, WM_MOUSEMOVE, WM_MOUSEWHEEL,
                WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETTINGCHANGE, WM_SIZE, WM_TIMER,
                WNDCLASSEXW, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
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
const WM_CONVERSION_CLOSED: u32 = crate::platform::conversion_dialog::WM_CONVERSION_CLOSED;
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
    export_formats: Vec<crate::imaging::ExportFormat>,
    selected_export_format: usize,
    active_path: Option<PathBuf>,
    catalog: FolderCatalog,
    cache: Vec<(PathBuf, ImageDocument)>,
    cache_bytes: usize,
    pending_catalog: Option<(u64, PathBuf)>,
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
    conversion_window: HWND,
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

mod implementation;

pub use implementation::{fatal_error, run};
