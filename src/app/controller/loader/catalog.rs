use super::*;

pub struct CatalogService {
    pending: Arc<(Mutex<Option<CatalogRequest>>, Condvar)>,
    current_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    results: Arc<Mailbox<CatalogPayload>>,
}

struct CatalogRequest {
    generation: u64,
    path: PathBuf,
    hwnd: usize,
}

impl CatalogService {
    pub fn new(window_alive: Arc<AtomicBool>) -> Self {
        let results = Arc::new(Mailbox::default());
        let worker_results = results.clone();
        let pending = Arc::new((Mutex::new(None::<CatalogRequest>), Condvar::new()));
        let current_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        let worker_generation = current_generation.clone();
        let worker_shutdown = shutdown.clone();

        let worker = thread::Builder::new()
            .name("kgen-catalog".into())
            .spawn(move || {
                loop {
                    let request = {
                        let (lock, wake) = &*worker_pending;
                        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        while slot.is_none() && !worker_shutdown.load(Ordering::Acquire) {
                            slot = wake
                                .wait(slot)
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                        }
                        if worker_shutdown.load(Ordering::Acquire) {
                            break;
                        }
                        slot.take()
                    };

                    let Some(request) = request else { continue };
                    let should_cancel = || {
                        worker_shutdown.load(Ordering::Acquire)
                            || worker_generation.load(Ordering::Acquire) != request.generation
                            || pending_has_work(&worker_pending)
                    };
                    let catalog = FolderCatalog::scan_cancellable(&request.path, &should_cancel);
                    if should_cancel() || !window_alive.load(Ordering::Acquire) {
                        continue;
                    }

                    if worker_results.publish(CatalogPayload {
                        generation: request.generation,
                        path: request.path,
                        catalog,
                    }) {
                        wake_window(request.hwnd, WM_CATALOG_DONE);
                    }
                }
            })
            .expect("start folder catalog worker");

        Self {
            pending,
            current_generation,
            shutdown,
            worker: Some(worker),
            results,
        }
    }

    pub fn take_result(&self) -> Option<CatalogPayload> {
        self.results.take()
    }

    pub fn submit(&self, generation: u64, path: PathBuf, hwnd: HWND) {
        self.current_generation.store(generation, Ordering::Release);
        let (lock, wake) = &*self.pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(CatalogRequest {
            generation,
            path,
            hwnd: hwnd.0 as usize,
        });
        wake.notify_one();
    }

    pub fn invalidate(&self, generation: u64) {
        self.current_generation.store(generation, Ordering::Release);
        let (lock, wake) = &*self.pending;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        wake.notify_one();
    }

    pub fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.results.close();
        self.pending
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.pending.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for CatalogService {
    fn drop(&mut self) {
        self.shutdown();
    }
}
