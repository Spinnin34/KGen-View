use super::*;

impl AppState {
    pub(super) fn new(settings: Settings) -> Result<Self, WinError> {
        let d2d_factory =
            unsafe { D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let alive = Arc::new(AtomicBool::new(true));
        Ok(Self {
            hwnd: HWND::default(),
            settings,
            document: None,
            export_formats: None,
            selected_export_format: 0,
            active_path: None,
            catalog: FolderCatalog::default(),
            cache: Vec::new(),
            cache_bytes: 0,
            pending_catalog: None,
            catalog_stamp: None,
            generation: 0,
            prefetch_enabled: true,
            navigation_direction: 1,
            alive: alive.clone(),
            loader: DecodeService::new(alive.clone()),
            file_service: FileService::new(),
            file_busy: false,
            pending_action: None,
            pending_properties: None,
            catalog_loader: CatalogService::new(alive),
            folder_watcher: FolderWatcher::new()?,
            folder_dirty: false,
            d2d_factory,
            target: None,
            bitmaps: Vec::new(),
            bitmap_size: (0, 0),
            first_visible: None,
            bitmap_dirty: true,
            view_request: 0,
            view_pending: false,
            requested_page: 0,
            view_key: None,
            upload_result: Arc::new(Mailbox::default()),
            update_events: Arc::new(Mailbox::default()),
            update_available: None,
            update_download_busy: false,
            chrome: Chrome::new(),
            status_model: StatusModel::default(),
            menu: None,
            conversion_window: HWND::default(),
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
            archive_sessions: Vec::new(),
            active_archive: None,
            archive_fallback: None,
            mcp_requests: None,
            pending_mcp_reply: None,
        })
    }

    pub(super) fn lang(&self) -> Language {
        self.settings.language
    }
}
