//! Coalescing background work for image decoding and folder catalog updates.
//!
//! Navigation can generate requests much faster than an image can be decoded.
//! Both services therefore keep only the newest pending request and use one
//! worker each. This bounds CPU and memory use while preserving the latest
//! navigation target.

mod catalog;
pub(super) use catalog::CatalogService;

use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL},
    },
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::{
    catalog::FolderCatalog,
    i18n::Language,
    imaging::{DecodeRequest, ImageDocument},
    mailbox::Mailbox,
};

use super::{
    CatalogPayload, LoadPayload, PrefetchPayload, ViewPayload, WM_CATALOG_DONE, WM_LOAD_DONE,
    WM_PREFETCH_DONE, WM_VIEW_DONE,
    archive::{ArchiveMemberRef, ArchiveSession, is_archive_path},
};

enum DecodeWork {
    Load {
        generation: u64,
        path: PathBuf,
        archive_member: Option<ArchiveMemberRef>,
        archive_member_name: Option<String>,
        hwnd: usize,
        language: Language,
        request: DecodeRequest,
    },
    View {
        generation: u64,
        request_id: u64,
        document: ImageDocument,
        request: DecodeRequest,
        hwnd: usize,
    },
}

pub(super) struct DecodeTarget {
    pub path: PathBuf,
    pub archive_member: Option<ArchiveMemberRef>,
}

struct PrefetchWork {
    generation: u64,
    targets: Vec<DecodeTarget>,
    hwnd: usize,
    language: Language,
}

pub(super) struct DecodeService {
    pending: Arc<(Mutex<Option<DecodeWork>>, Condvar)>,
    prefetch_pending: Arc<(Mutex<Option<PrefetchWork>>, Condvar)>,
    current_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    suspended: Arc<AtomicBool>,
    foreground_active: Arc<AtomicBool>,
    loads: Arc<Mailbox<LoadPayload>>,
    prefetches: Arc<Mailbox<PrefetchPayload>>,
    views: Arc<Mailbox<ViewPayload>>,
}

impl DecodeService {
    pub fn new(window_alive: Arc<AtomicBool>) -> Self {
        let suspended = Arc::new(AtomicBool::new(false));
        let foreground_active = Arc::new(AtomicBool::new(false));
        let worker_active = foreground_active.clone();
        let loads = Arc::new(Mailbox::default());
        let prefetches = Arc::new(Mailbox::default());
        let views = Arc::new(Mailbox::default());
        let worker_loads = loads.clone();
        let worker_views = views.clone();
        let pending = Arc::new((Mutex::new(None::<DecodeWork>), Condvar::new()));
        let prefetch_pending = Arc::new((Mutex::new(None::<PrefetchWork>), Condvar::new()));
        let current_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        let worker_generation = current_generation.clone();
        let worker_shutdown = shutdown.clone();
        let worker_alive = window_alive.clone();

        let decode_worker = thread::Builder::new()
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
                    worker_active.store(true,Ordering::Release);
                    let _active=ActiveWork(worker_active.clone());
                    match work {
                        DecodeWork::Load {
                            generation,
                            path,
                            archive_member,
                            archive_member_name,
                            hwnd,
                            language,
                            request,
                        } => {
                            let should_cancel = || {
                                worker_shutdown.load(Ordering::Acquire)
                                    || worker_generation.load(Ordering::Acquire) != generation
                                    || pending_has_work(&worker_pending)
                            };
                            let decode = |path: &std::path::Path| ImageDocument::decode_request(path,&should_cancel,wic_factory.as_ref(),&request);
                            let (loaded_path, result, catalog, archive) = if is_archive_path(&path)
                            {
                                match ArchiveSession::open(&path, &should_cancel, language) {
                                    Ok(archive) => {
                                        let member = match archive_member_name.as_deref() {
                                            Some(name) => archive.member_ref_by_name(name),
                                            None => archive.first_member(),
                                        };
                                        if let Some(member) = member {
                                            let image_path = member.path().to_path_buf();
                                            let lease=member.lease();
                                            let result=member.extract(&should_cancel,language).and_then(|()|decode(&image_path)).map(|mut document| { document.source_lease=Some(lease); document });
                                            // Archive navigation is driven by the session's sorted
                                            // member list. Do not clone it into a folder catalog
                                            // keyed by extracted temporary paths.
                                            (image_path, result, FolderCatalog::default(), Some(archive))
                                        } else {
                                            let error = if language == Language::Spanish {
                                                "No se encontró la imagen seleccionada dentro del ZIP."
                                            } else {
                                                "The selected image was not found inside the ZIP archive."
                                            };
                                            (
                                                path,
                                                Err(crate::imaging::ImageError(error.into())),
                                                FolderCatalog::default(),
                                                None,
                                            )
                                        }
                                    }
                                    Err(error) => {
                                        (path, Err(error), FolderCatalog::default(), None)
                                    }
                                }
                            } else {
                                let archive_member_load = archive_member.is_some();
                                let result = match archive_member {
                                    Some(member) => {
                                        let lease=member.lease();
                                        member.extract(&should_cancel,language).and_then(|()|decode(&path)).map(|mut document| { document.source_lease=Some(lease); document })
                                    },
                                    None => decode(&path),
                                };
                                // A failed external load still needs a catalog so the
                                // UI can skip to the next valid image without scanning
                                // the folder on the UI thread.
                                let catalog = if result.is_err() && !archive_member_load {
                                    FolderCatalog::scan_cancellable(&path, &should_cancel)
                                } else {
                                    FolderCatalog::default()
                                };
                                (path, result, catalog, None)
                            };
                            if should_cancel() {
                                continue;
                            }
                            worker_active.store(false,Ordering::Release);
                            if !post_load(
                                &worker_loads, &worker_alive,
                                hwnd,
                                LoadPayload {
                                    generation,
                                    path: loaded_path,
                                    result,
                                    catalog,
                                    archive,
                                },
                            ) {
                                break;
                            }
                        }
                        DecodeWork::View { generation, request_id, document, request, hwnd } => {
                            let should_cancel = || worker_shutdown.load(Ordering::Acquire)
                                || worker_generation.load(Ordering::Acquire) != generation
                                || pending_has_work(&worker_pending);
                            let result = document.render_view(request,&should_cancel,wic_factory.as_ref());
                            worker_active.store(false,Ordering::Release);
                            if !should_cancel() && worker_alive.load(Ordering::Acquire)
                                && worker_views.publish(ViewPayload { generation,request_id,result }) {
                                wake_window(hwnd,WM_VIEW_DONE);
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
        let worker_prefetches = prefetches.clone();
        let worker_suspended = suspended.clone();
        let worker_foreground = foreground_active.clone();
        let prefetch_worker = thread::Builder::new()
            .name("kgen-prefetch".into())
            .spawn(move || {
                // Speculative decoding should give way immediately to the
                // foreground decode and UI when both are runnable.
                let _ =
                    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
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
                    for target in request.targets {
                        let DecodeTarget {
                            path,
                            archive_member,
                        } = target;
                        let should_cancel = || {
                            worker_suspended.load(Ordering::Acquire)
                                || worker_foreground.load(Ordering::Acquire)
                                || worker_shutdown.load(Ordering::Acquire)
                                || worker_generation.load(Ordering::Acquire) != request.generation
                                || pending_has_work(&worker_pending)
                                || pending_has_work(&worker_prefetch_pending)
                        };
                        if should_cancel() {
                            break;
                        }
                        let lease = archive_member.as_ref().map(|member| member.lease());
                        if let Some(member) = archive_member
                            && member.extract(&should_cancel, request.language).is_err()
                        {
                            if should_cancel() {
                                break;
                            }
                            continue;
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
                            &worker_prefetches,
                            &window_alive,
                            request.hwnd,
                            PrefetchPayload {
                                generation: request.generation,
                                path,
                                result: result.map(|mut document| {
                                    document.source_lease = lease
                                        .map(|lease| lease as Arc<dyn crate::imaging::SourceLease>);
                                    document
                                }),
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
            workers: vec![decode_worker, prefetch_worker],
            loads,
            prefetches,
            views,
            suspended,
            foreground_active,
        }
    }

    pub fn submit(
        &self,
        generation: u64,
        path: PathBuf,
        archive_member: Option<ArchiveMemberRef>,
        archive_member_name: Option<String>,
        hwnd: HWND,
        language: Language,
        request: DecodeRequest,
    ) {
        self.current_generation.store(generation, Ordering::Release);
        self.replace(DecodeWork::Load {
            generation,
            path,
            archive_member,
            archive_member_name,
            hwnd: hwnd.0 as usize,
            language,
            request,
        });
        // A foreground navigation invalidates queued look-ahead immediately.
        // An active prefetch observes the pending load through its cancellation
        // predicate and exits at the next decode checkpoint.
        let (lock, wake) = &*self.prefetch_pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = None;
        wake.notify_one();
    }

    pub fn set_suspended(&self, value: bool) {
        self.suspended.store(value, Ordering::Release);
        if value {
            self.prefetch_pending
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            self.prefetches.take();
        }
    }

    pub fn take_load(&self) -> Option<LoadPayload> {
        self.loads.take()
    }
    pub fn take_prefetch(&self) -> Option<PrefetchPayload> {
        self.prefetches.take()
    }
    pub fn take_view(&self) -> Option<ViewPayload> {
        self.views.take()
    }

    pub fn submit_view(
        &self,
        generation: u64,
        request_id: u64,
        document: ImageDocument,
        request: DecodeRequest,
        hwnd: HWND,
    ) {
        self.replace(DecodeWork::View {
            generation,
            request_id,
            document,
            request,
            hwnd: hwnd.0 as usize,
        });
    }

    pub fn submit_prefetch(
        &self,
        generation: u64,
        targets: Vec<DecodeTarget>,
        hwnd: HWND,
        language: Language,
    ) {
        if self.suspended.load(Ordering::Acquire)
            || self.foreground_active.load(Ordering::Acquire)
            || targets.is_empty()
            || self.current_generation.load(Ordering::Acquire) != generation
        {
            return;
        }
        let (lock, wake) = &*self.prefetch_pending;
        let mut slot = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(PrefetchWork {
            generation,
            targets,
            hwnd: hwnd.0 as usize,
            language,
        });
        wake.notify_one();
    }

    pub fn invalidate(&self, generation: u64) {
        self.current_generation.store(generation, Ordering::Release);
        self.loads.take();
        self.views.take();
        self.prefetches.take();
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

    pub fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.loads.close();
        self.views.close();
        self.prefetches.close();
        self.pending
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.prefetch_pending
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.pending.1.notify_one();
        self.prefetch_pending.1.notify_one();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for DecodeService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pending_has_work<T>(pending: &Arc<(Mutex<Option<T>>, Condvar)>) -> bool {
    pending
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_some()
}

fn post_load(
    inbox: &Mailbox<LoadPayload>,
    alive: &AtomicBool,
    hwnd: usize,
    payload: LoadPayload,
) -> bool {
    if !alive.load(Ordering::Acquire) {
        return false;
    }
    if !inbox.publish(payload) {
        return false;
    }
    wake_window(hwnd, WM_LOAD_DONE);
    true
}

fn post_prefetch(
    inbox: &Mailbox<PrefetchPayload>,
    alive: &AtomicBool,
    hwnd: usize,
    payload: PrefetchPayload,
) -> bool {
    if !alive.load(Ordering::Acquire) {
        return false;
    }
    if !inbox.publish(payload) {
        return false;
    }
    wake_window(hwnd, WM_PREFETCH_DONE);
    true
}

fn wake_window(hwnd: usize, message: u32) {
    let _ = unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut c_void)),
            message,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

struct ActiveWork(Arc<AtomicBool>);
impl Drop for ActiveWork {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_navigation_keeps_the_latest_generation_and_shuts_down_cleanly() {
        let alive = Arc::new(AtomicBool::new(true));
        let mut loader = DecodeService::new(alive.clone());
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/kgen-view-logo.png");
        for generation in 1..=1000 {
            loader.invalidate(generation);
            loader.submit(
                generation,
                path.clone(),
                None,
                None,
                HWND::default(),
                Language::Spanish,
                DecodeRequest::preview((64, 64)),
            );
        }
        let start = std::time::Instant::now();
        loop {
            if let Some(payload) = loader.take_load()
                && payload.generation == 1000
            {
                assert!(payload.result.is_ok());
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "latest navigation did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        alive.store(false, Ordering::Release);
        loader.shutdown();
        assert!(loader.take_load().is_none());
        assert!(loader.take_prefetch().is_none());
        assert!(loader.take_view().is_none());
    }
}
