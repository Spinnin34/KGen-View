impl AppState {
    fn new(settings: Settings) -> Result<Self, WinError> {
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
            catalog_loader: CatalogService::new(alive),
            d2d_factory,
            target: None,
            bitmap: None,
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
        })
    }

    fn lang(&self) -> Language {
        self.settings.language
    }
}
