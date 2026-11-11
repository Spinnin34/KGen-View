//! One cancellable, overlapped directory watch; no polling while supported.
use crate::{imaging::wide_path, mailbox::Mailbox};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadDirectoryChangesW,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects},
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::PCWSTR,
};

pub(super) struct FolderEvent {
    pub folder: PathBuf,
    pub supported: bool,
}
pub(super) struct FolderWatcher {
    pending: Arc<Mutex<Option<Option<PathBuf>>>>,
    results: Arc<Mailbox<FolderEvent>>,
    stop: usize,
    changed: usize,
    worker: Option<JoinHandle<()>>,
    folder: Option<PathBuf>,
}

struct Watch {
    directory: HANDLE,
    event: HANDLE,
    buffer: Box<[u32; 8192]>,
    overlapped: Box<OVERLAPPED>,
    pending: bool,
}
impl Watch {
    fn open(folder: &std::path::Path) -> windows::core::Result<Self> {
        let path = wide_path(folder);
        let directory = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )?
        };
        let event = match unsafe { CreateEventW(None, true, false, None) } {
            Ok(event) => event,
            Err(error) => {
                unsafe {
                    let _ = CloseHandle(directory);
                }
                return Err(error);
            }
        };
        let mut watch = Self {
            directory,
            event,
            buffer: Box::new([0; 8192]),
            overlapped: Box::new(OVERLAPPED {
                hEvent: event,
                ..Default::default()
            }),
            pending: false,
        };
        watch.arm()?;
        Ok(watch)
    }
    fn arm(&mut self) -> windows::core::Result<()> {
        unsafe {
            ResetEvent(self.event)?;
        }
        *self.overlapped = OVERLAPPED {
            hEvent: self.event,
            ..Default::default()
        };
        unsafe {
            ReadDirectoryChangesW(
                self.directory,
                self.buffer.as_mut_ptr().cast(),
                (self.buffer.len() * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
                None,
                Some(&mut *self.overlapped),
                None,
            )?;
        }
        self.pending = true;
        Ok(())
    }
    fn finish(&mut self) {
        let mut count = 0;
        // Zero bytes/overflow also trigger a full rescan, so no events are lost.
        let _ =
            unsafe { GetOverlappedResult(self.directory, &*self.overlapped, &mut count, false) };
        self.pending = false;
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            if self.pending {
                let _ = CancelIoEx(self.directory, Some(&*self.overlapped));
                let mut count = 0;
                let _ = GetOverlappedResult(self.directory, &*self.overlapped, &mut count, true);
            }
            let _ = CloseHandle(self.event);
            let _ = CloseHandle(self.directory);
        }
    }
}

impl FolderWatcher {
    pub fn new() -> windows::core::Result<Self> {
        let stop = unsafe { CreateEventW(None, true, false, None)? }.0 as usize;
        let changed = match unsafe { CreateEventW(None, false, false, None) } {
            Ok(event) => event.0 as usize,
            Err(error) => {
                unsafe {
                    let _ = CloseHandle(HANDLE(stop as *mut _));
                }
                return Err(error);
            }
        };
        Ok(Self {
            pending: Arc::new(Mutex::new(None)),
            results: Arc::new(Mailbox::default()),
            stop,
            changed,
            worker: None,
            folder: None,
        })
    }
    pub fn start(&mut self, hwnd: HWND) {
        if let Some(worker) = self.worker.take() {
            if !worker.is_finished() {
                self.worker = Some(worker);
                return;
            }
            let _ = worker.join();
        }
        let (stop, changed, hwnd) = (self.stop, self.changed, hwnd.0 as usize);
        let (pending, results) = (self.pending.clone(), self.results.clone());
        self.worker = Some(
            thread::Builder::new()
                .name("kgen-folder-watch".into())
                .spawn(move || {
                    let mut watch: Option<Watch> = None;
                    let mut folder: Option<PathBuf> = None;
                    let mut dirty: Option<Instant> = None;
                    let publish = |folder: PathBuf, supported: bool| {
                        if results.publish(FolderEvent { folder, supported }) {
                            let _ = unsafe {
                                PostMessageW(
                                    Some(HWND(hwnd as *mut _)),
                                    super::WM_FOLDER_CHANGED,
                                    WPARAM(0),
                                    LPARAM(0),
                                )
                            };
                        }
                    };
                    loop {
                        let mut handles = vec![HANDLE(stop as *mut _), HANDLE(changed as *mut _)];
                        if let Some(watch) = &watch {
                            handles.push(watch.event);
                        }
                        let timeout = dirty
                            .map(|started| {
                                Duration::from_millis(200)
                                    .saturating_sub(started.elapsed())
                                    .as_millis() as u32
                            })
                            .unwrap_or(INFINITE);
                        let event = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
                        if event == WAIT_OBJECT_0 {
                            break;
                        }
                        if event.0 == WAIT_OBJECT_0.0 + 1 {
                            let requested =
                                pending.lock().unwrap_or_else(|e| e.into_inner()).take();
                            if let Some(requested) = requested {
                                watch = None;
                                dirty = None;
                                folder = requested;
                                if let Some(path) = &folder {
                                    match Watch::open(path) {
                                        Ok(opened) => {
                                            watch = Some(opened);
                                            publish(path.clone(), true);
                                        }
                                        Err(_) => publish(path.clone(), false),
                                    }
                                }
                            }
                        } else if event.0 == WAIT_OBJECT_0.0 + 2 {
                            if let Some(active) = &mut watch {
                                active.finish();
                                if active.arm().is_err() {
                                    watch = None;
                                    if let Some(path) = &folder {
                                        publish(path.clone(), false);
                                    }
                                }
                                dirty.get_or_insert_with(Instant::now);
                            }
                        } else if event == WAIT_TIMEOUT {
                            if dirty.take().is_some()
                                && let Some(path) = &folder
                            {
                                publish(path.clone(), watch.is_some());
                            }
                        } else {
                            if let Some(path) = &folder {
                                publish(path.clone(), false);
                            }
                            break;
                        }
                    }
                })
                .expect("start directory watch worker"),
        );
    }
    pub fn watch(&mut self, folder: Option<PathBuf>) {
        if folder == self.folder {
            return;
        }
        self.folder = folder.clone();
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(folder);
        unsafe {
            let _ = SetEvent(HANDLE(self.changed as *mut _));
        }
    }
    pub fn take(&self) -> Option<FolderEvent> {
        self.results.take()
    }
    pub fn shutdown(&mut self) {
        self.results.close();
        unsafe {
            let _ = SetEvent(HANDLE(self.stop as *mut _));
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.shutdown();
        unsafe {
            let _ = CloseHandle(HANDLE(self.stop as *mut _));
            let _ = CloseHandle(HANDLE(self.changed as *mut _));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::WaitForSingleObject;
    #[test]
    fn directory_notifications_rearm_and_pending_reads_cancel_cleanly() {
        let directory =
            std::env::temp_dir().join(format!("kgen-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("test.png");
        let mut watch = Watch::open(&directory).unwrap();
        std::fs::write(&file, b"first").unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(watch.event, 3000) },
            WAIT_OBJECT_0
        );
        watch.finish();
        watch.arm().unwrap();
        std::fs::write(&file, b"second content").unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(watch.event, 3000) },
            WAIT_OBJECT_0
        );
        watch.finish();
        watch.arm().unwrap();
        let started = Instant::now();
        drop(watch);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
