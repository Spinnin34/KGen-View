//! Serialized explicit file operations. Requests are never silently replaced.
use crate::{
    imaging::{DecodeRequest, DocumentId, ExportFormat, ImageDocument},
    mailbox::Mailbox,
    platform::clipboard::PreparedImage,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    UI::WindowsAndMessaging::PostMessageW,
};

pub(super) enum FileTask {
    Save {
        document: ImageDocument,
        path: PathBuf,
        format: ExportFormat,
    },
    ExportCopy {
        document: ImageDocument,
        path: PathBuf,
        format: ExportFormat,
    },
    Copy(ImageDocument),
    Delete(PathBuf),
    Rename {
        id: DocumentId,
        from: PathBuf,
        to: PathBuf,
    },
}

pub(super) enum FileResult {
    Saved {
        id: DocumentId,
        revision: u64,
        result: Result<Box<ImageDocument>, String>,
    },
    Exported {
        path: PathBuf,
        result: Result<(), String>,
    },
    Copied(Result<PreparedImage, String>),
    Renamed {
        id: DocumentId,
        path: PathBuf,
        result: Result<(), String>,
    },
    Deleted {
        path: PathBuf,
        result: Result<bool, String>,
    },
}

pub(super) struct FileService {
    pending: Arc<(Mutex<Option<FileTask>>, Condvar)>,
    stopping: Arc<AtomicBool>,
    results: Arc<Mailbox<FileResult>>,
    worker: Option<JoinHandle<()>>,
    busy: AtomicBool,
}

impl FileService {
    pub fn new() -> Self {
        let pending = Arc::new((Mutex::new(None), Condvar::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let results = Arc::new(Mailbox::default());
        Self {
            pending,
            stopping,
            results,
            worker: None,
            busy: AtomicBool::new(false),
        }
    }
    pub fn start(&mut self, hwnd: HWND) {
        let (pending, stopping, results) = (
            self.pending.clone(),
            self.stopping.clone(),
            self.results.clone(),
        );
        let hwnd = hwnd.0 as usize;
        self.worker = Some(
            thread::Builder::new()
                .name("kgen-file-operations".into())
                .spawn(move || {
                    let initialized =
                        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
                    loop {
                        let task = {
                            let mut slot = pending.0.lock().unwrap_or_else(|e| e.into_inner());
                            while slot.is_none() && !stopping.load(Ordering::Acquire) {
                                slot = pending.1.wait(slot).unwrap_or_else(|e| e.into_inner());
                            }
                            if stopping.load(Ordering::Acquire) {
                                break;
                            }
                            slot.take().expect("file task")
                        };
                        let result = match task {
                            FileTask::Save {
                                mut document,
                                path,
                                format,
                            } => {
                                let (id, revision) = (document.id, document.revision);
                                let result = if initialized {
                                    document
                                        .save_as_format(&path, &format)
                                        .map(|()| Box::new(document))
                                        .map_err(|e| e.to_string())
                                } else {
                                    Err("No se pudo inicializar Windows Imaging Component.".into())
                                };
                                FileResult::Saved {
                                    id,
                                    revision,
                                    result,
                                }
                            }
                            FileTask::ExportCopy {
                                document,
                                path,
                                format,
                            } => {
                                let result = if initialized {
                                    document
                                        .export_copy_as(&path, &format)
                                        .map_err(|error| error.to_string())
                                } else {
                                    Err("No se pudo inicializar Windows Imaging Component.".into())
                                };
                                FileResult::Exported { path, result }
                            }
                            FileTask::Copy(document) => {
                                let result = document
                                    .render_view(
                                        DecodeRequest::original(document.frame_index),
                                        &|| stopping.load(Ordering::Acquire),
                                        None,
                                    )
                                    .and_then(|document| {
                                        let frame = document.current_frame();
                                        PreparedImage::new(frame.width, frame.height, &frame.pixels)
                                            .map_err(crate::imaging::ImageError)
                                    })
                                    .map_err(|e| e.to_string());
                                FileResult::Copied(result)
                            }
                            FileTask::Rename { id, from, to } => {
                                let from = crate::imaging::wide_path(&from);
                                let wide_to = crate::imaging::wide_path(&to);
                                let result = unsafe {
                                    windows::Win32::Storage::FileSystem::MoveFileExW(
                                        windows::core::PCWSTR(from.as_ptr()),
                                        windows::core::PCWSTR(wide_to.as_ptr()),
                                        windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS(0),
                                    )
                                }
                                .map_err(|e| e.to_string());
                                FileResult::Renamed {
                                    id,
                                    path: to,
                                    result,
                                }
                            }
                            FileTask::Delete(path) => {
                                let mut from = crate::imaging::wide_path(&path);
                                from.push(0);
                                use windows::Win32::UI::Shell::{
                                    FO_DELETE, FOF_ALLOWUNDO, FOF_NOERRORUI, SHFILEOPSTRUCTW,
                                    SHFileOperationW,
                                };
                                let mut operation = SHFILEOPSTRUCTW {
                                    wFunc: FO_DELETE,
                                    pFrom: windows::core::PCWSTR(from.as_ptr()),
                                    fFlags: (FOF_ALLOWUNDO.0 | FOF_NOERRORUI.0) as u16,
                                    ..Default::default()
                                };
                                let code = unsafe { SHFileOperationW(&mut operation) };
                                let result = if operation.fAnyOperationsAborted.as_bool() {
                                    Ok(false)
                                } else if code == 0 {
                                    Ok(true)
                                } else {
                                    Err(std::io::Error::from_raw_os_error(code).to_string())
                                };
                                FileResult::Deleted { path, result }
                            }
                        };
                        if !stopping.load(Ordering::Acquire) && results.publish(result) {
                            let _ = unsafe {
                                PostMessageW(
                                    Some(HWND(hwnd as *mut _)),
                                    super::WM_FILE_DONE,
                                    WPARAM(0),
                                    LPARAM(0),
                                )
                            };
                        }
                    }
                    if initialized {
                        unsafe {
                            CoUninitialize();
                        }
                    }
                })
                .expect("start file worker"),
        );
    }
    pub fn submit(&self, task: FileTask) -> bool {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let mut slot = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() || self.stopping.load(Ordering::Acquire) {
            self.busy.store(false, Ordering::Release);
            return false;
        }
        *slot = Some(task);
        self.pending.1.notify_one();
        true
    }
    pub fn take(&self) -> Option<FileResult> {
        let result = self.results.take();
        if result.is_some() {
            self.busy.store(false, Ordering::Release);
        }
        result
    }
    pub fn shutdown(&mut self) {
        self.stopping.store(true, Ordering::Release);
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
impl Drop for FileService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_worker_preserves_resolution_and_rejects_overlapping_commands() {
        let initialized =
            unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) }
                .is_ok();
        let source =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/kgen-view-logo.png");
        let output =
            std::env::temp_dir().join(format!("kgen-save-worker-{}.png", std::process::id()));
        let document = ImageDocument::decode_request(
            &source,
            &|| false,
            None,
            &DecodeRequest::preview((16, 16)),
        )
        .unwrap();
        let size = document.original_size;
        let id = document.id;
        let format = ExportFormat {
            container: windows::Win32::Graphics::Imaging::GUID_ContainerFormatPng,
            name: "PNG".into(),
            extensions: vec!["png".into()],
        };
        let mut worker = FileService::new();
        worker.start(HWND::default());
        assert!(worker.submit(FileTask::Save {
            document: document.clone(),
            path: output.clone(),
            format
        }));
        assert!(!worker.submit(FileTask::Copy(document)));
        let start = std::time::Instant::now();
        let result = loop {
            if let Some(result) = worker.take() {
                break result;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        match result {
            FileResult::Saved {
                result: Ok(saved), ..
            } => {
                assert_eq!(saved.id, id);
                assert!(!saved.dirty);
                assert_eq!(saved.original_size, size);
            }
            _ => panic!("save failed"),
        }
        let exported =
            ImageDocument::decode_request(&output, &|| false, None, &DecodeRequest::original(0))
                .unwrap();
        assert_eq!(exported.original_size, size);
        worker.shutdown();
        std::fs::remove_file(output).unwrap();
        if initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
