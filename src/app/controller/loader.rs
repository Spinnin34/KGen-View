//! Coalescing background work for image decoding and folder catalog updates.
//!
//! Navigation can generate requests much faster than an image can be decoded.
//! Both services therefore keep only the newest pending request and use one
//! worker each. This bounds CPU and memory use while preserving the latest
//! navigation target.

use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::{catalog::FolderCatalog, imaging::ImageDocument};

use super::{
    CatalogPayload, LoadPayload, PrefetchPayload, WM_CATALOG_DONE, WM_LOAD_DONE, WM_PREFETCH_DONE,
};

enum DecodeWork {
    Load {
        generation: u64,
        path: PathBuf,
        hwnd: usize,
    },
}

struct PrefetchWork {
    generation: u64,
    paths: Vec<PathBuf>,
    hwnd: usize,
}

pub(super) struct DecodeService {
    pending: Arc<(Mutex<Option<DecodeWork>>, Condvar)>,
    prefetch_pending: Arc<(Mutex<Option<PrefetchWork>>, Condvar)>,
    current_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl DecodeService {
    pub fn new(window_alive: Arc<AtomicBool>) -> Self {
        let pending = Arc::new((Mutex::new(None::<DecodeWork>), Condvar::new()));
        let prefetch_pending = Arc::new((Mutex::new(None::<PrefetchWork>), Condvar::new()));
        let current_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        let worker_generation = current_generation.clone();
        let worker_shutdown = shutdown.clone();
        let worker_alive = window_alive.clone();

        thread::Builder::new()
            .name("kgen-decode".into())
            .spawn(move || {
                let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                let wic_factory = ImageDocument::create_wic_factory().ok();
                loop {
                    let work = {
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

                    let Some(work) = work else { continue };
                    match work {
                        DecodeWork::Load {
                            generation,
                            path,
                            hwnd,
                        } => {
                            let should_cancel = || {
                                worker_shutdown.load(Ordering::Acquire)
                                    || worker_generation.load(Ordering::Acquire) != generation
                                    || pending_has_work(&worker_pending)
                            };
                            let result = match wic_factory.as_ref() {
                                Some(factory) => ImageDocument::decode_cancellable_with_factory(
                                    &path,
                                    &should_cancel,
                                    factory,
                                ),
                                None => ImageDocument::decode_cancellable(&path, &should_cancel),
                            };
                            if should_cancel() {
                                continue;
                            }
                            // A failed external load still needs a catalog so the
                            // UI can skip to the next valid image without scanning
                            // the folder on the UI thread.
                            let catalog = if result.is_err() {
                                FolderCatalog::scan_cancellable(&path, &should_cancel)
                            } else {
                                FolderCatalog::default()
                            };
                            if should_cancel() {
                                continue;
                            }
                            if !post_load(
                                &worker_alive,
                                hwnd,
                                LoadPayload {
                                    generation,
                                    path,
                                    result,
                                    catalog,
                                },
                            ) {
                                break;
                            }
                        }
                    }
                }
                if initialized {
                    unsafe { CoUninitialize() };
                }
            })
            .expect("start image decode worker");

        let worker_pending = pending.clone();
        let worker_prefetch_pending = prefetch_pending.clone();
        let worker_generation = current_generation.clone();
        let worker_shutdown = shutdown.clone();
        thread::Builder::new()
            .name("kgen-prefetch".into())
            .spawn(move || {
                let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                let wic_factory = ImageDocument::create_wic_factory().ok();
                loop {
                    let request = {
                        let (lock, wake) = &*worker_prefetch_pending;
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
                    for path in request.paths {
                        let should_cancel = || {
                            worker_shutdown.load(Ordering::Acquire)
                                || worker_generation.load(Ordering::Acquire) != request.generation
                                || pending_has_work(&worker_pending)
                                || pending_has_work(&worker_prefetch_pending)
                        };
                        if should_cancel() {
                            break;
                        }
                        let Some(result) = ImageDocument::decode_prefetch_cancellable(
                            &path,
                            &should_cancel,
                            wic_factory.as_ref(),
                        ) else {
                            if should_cancel() {
                                break;
                            }
                            continue;
                        };
                        if should_cancel() {
                            break;
                        }
                        if result.is_err() {
                            continue;
                        }
                        if !post_prefetch(
                            &window_alive,
                            request.hwnd,
                            PrefetchPayload {
                                generation: request.generation,
                                path,
                                result,
                            },
                        ) {
                            break;
                        }
                    }
                }
                if initialized {
                    unsafe { CoUninitialize() };
                }
            })
            .expect("start image prefetch worker");

        Self {
            pending,
            prefetch_pending,
            current_generation,
            shutdown,
        }
    }

    pub fn submit(&self, generation: u64, path: PathBuf, hwnd: HWND) {
        self.current_generation.store(generation, Ordering::Release);
        self.replace(DecodeWork::Load {
            generation,
            path,
            hwnd: hwnd.0 as usize,
        });
        // A foreground navigation invalidates queued look-ahead immediately.
        // An active prefetch observes the pending load through its cancellation
        // predicate and exits at the next decode checkpoint.
        let (lock, wake) = &*self.prefetch_pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = None;
        wake.notify_one();
    }

    pub fn submit_prefetch(&self, generation: u64, paths: Vec<PathBuf>, hwnd: HWND) {
        if paths.is_empty() || self.current_generation.load(Ordering::Acquire) != generation {
            return;
        }
        let (lock, wake) = &*self.prefetch_pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(PrefetchWork {
            generation,
            paths,
            hwnd: hwnd.0 as usize,
        });
        wake.notify_one();
    }

    pub fn invalidate(&self, generation: u64) {
        self.current_generation.store(generation, Ordering::Release);
        let (lock, wake) = &*self.pending;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        wake.notify_one();
        let (lock, wake) = &*self.prefetch_pending;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        wake.notify_one();
    }

    fn replace(&self, work: DecodeWork) {
        let (lock, wake) = &*self.pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(work);
        wake.notify_one();
    }
}

impl Drop for DecodeService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.pending.1.notify_one();
        self.prefetch_pending.1.notify_one();
    }
}

pub(super) struct CatalogService {
    pending: Arc<(Mutex<Option<CatalogRequest>>, Condvar)>,
    current_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

struct CatalogRequest {
    generation: u64,
    path: PathBuf,
    hwnd: usize,
}

impl CatalogService {
    pub fn new(window_alive: Arc<AtomicBool>) -> Self {
        let pending = Arc::new((Mutex::new(None::<CatalogRequest>), Condvar::new()));
        let current_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        let worker_generation = current_generation.clone();
        let worker_shutdown = shutdown.clone();

        thread::Builder::new()
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

                    let payload = Box::new(CatalogPayload {
                        generation: request.generation,
                        path: request.path,
                        catalog,
                    });
                    let raw = Box::into_raw(payload);
                    let hwnd = HWND(request.hwnd as *mut c_void);
                    if unsafe {
                        PostMessageW(Some(hwnd), WM_CATALOG_DONE, WPARAM(0), LPARAM(raw as isize))
                    }
                    .is_err()
                    {
                        unsafe { drop(Box::from_raw(raw)) };
                    }
                }
            })
            .expect("start folder catalog worker");

        Self {
            pending,
            current_generation,
            shutdown,
        }
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
}

impl Drop for CatalogService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.pending.1.notify_one();
    }
}

fn pending_has_work<T>(pending: &Arc<(Mutex<Option<T>>, Condvar)>) -> bool {
    pending
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_some()
}

fn post_load(alive: &AtomicBool, hwnd_raw: usize, payload: LoadPayload) -> bool {
    if !alive.load(Ordering::Acquire) {
        return false;
    }
    let raw = Box::into_raw(Box::new(payload));
    let hwnd = HWND(hwnd_raw as *mut c_void);
    if unsafe { PostMessageW(Some(hwnd), WM_LOAD_DONE, WPARAM(0), LPARAM(raw as isize)) }.is_err() {
        unsafe { drop(Box::from_raw(raw)) };
        return false;
    }
    true
}

fn post_prefetch(alive: &AtomicBool, hwnd_raw: usize, payload: PrefetchPayload) -> bool {
    if !alive.load(Ordering::Acquire) {
        return false;
    }
    let raw = Box::into_raw(Box::new(payload));
    let hwnd = HWND(hwnd_raw as *mut c_void);
    if unsafe {
        PostMessageW(
            Some(hwnd),
            WM_PREFETCH_DONE,
            WPARAM(0),
            LPARAM(raw as isize),
        )
    }
    .is_err()
    {
        unsafe { drop(Box::from_raw(raw)) };
        return false;
    }
    true
}
