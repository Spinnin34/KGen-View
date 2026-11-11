//! Opt-in local measurements. No file paths, image metadata, or network traffic.
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("KGEN_PERF").is_some_and(|value| value != "0"))
}

pub fn elapsed(stage: &str, duration: Duration) {
    if enabled() {
        eprintln!(
            "KGEN_PERF stage={stage} ms={:.3} cpu_bytes={} cpu_peak={}",
            duration.as_secs_f64() * 1000.0,
            crate::resources::cpu_budget().used(),
            crate::resources::cpu_budget().peak()
        );
    }
}

pub struct Span {
    stage: &'static str,
    start: Instant,
}
impl Span {
    pub fn new(stage: &'static str) -> Self {
        Self {
            stage,
            start: Instant::now(),
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        elapsed(self.stage, self.start.elapsed());
    }
}

/// Developer-only CLI probe; each invocation measures a fresh process.
pub fn benchmark_cli() -> Option<Result<(), String>> {
    let arguments: Vec<_> = std::env::args_os().collect();
    let mode = arguments.get(1)?.to_str()?;
    if mode != "--benchmark-image" && mode != "--benchmark-catalog" {
        return None;
    }
    Some((|| {
        use windows::Win32::System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
            ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
            },
            Threading::GetCurrentProcess,
        };
        let path = arguments
            .get(2)
            .map(std::path::PathBuf::from)
            .ok_or("Falta la ruta del archivo o catálogo.")?;
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        let result = (|| {
            let (width, height, original_width, original_height, pixel_bytes, files) = if mode
                == "--benchmark-image"
            {
                let request = if arguments.iter().any(|arg| arg == "--original") {
                    crate::imaging::DecodeRequest::original(0)
                } else {
                    crate::imaging::DecodeRequest::preview((1920, 1080))
                };
                let document =
                    crate::imaging::ImageDocument::decode_request(&path, &|| false, None, &request)
                        .map_err(|e| e.to_string())?;
                let frame = document.current_frame();
                (
                    frame.width,
                    frame.height,
                    document.original_size.0,
                    document.original_size.1,
                    frame.pixels.len(),
                    0,
                )
            } else {
                let catalog = crate::catalog::FolderCatalog::scan_cancellable(&path, &|| false);
                (0, 0, 0, 0, 0, catalog.files.len())
            };
            let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
            let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
            memory.cb = std::mem::size_of_val(&memory) as u32;
            unsafe {
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX)
                        .cast::<PROCESS_MEMORY_COUNTERS>(),
                    std::mem::size_of_val(&memory) as u32,
                )
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{{\"elapsed_ms\":{milliseconds:.3},\"width\":{width},\"height\":{height},\"original_width\":{original_width},\"original_height\":{original_height},\"pixel_bytes\":{pixel_bytes},\"files\":{files},\"private_bytes\":{},\"peak_working_set_bytes\":{},\"managed_peak_bytes\":{}}}",
                memory.PrivateUsage,
                memory.PeakWorkingSetSize,
                crate::resources::cpu_budget().peak()
            );
            Ok(())
        })();
        unsafe {
            CoUninitialize();
        }
        result
    })())
}
