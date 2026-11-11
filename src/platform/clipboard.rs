use std::ptr;

use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HWND},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::{CF_DIB, CF_UNICODETEXT},
    },
};

use super::text::wide;

pub fn copy_text(hwnd: HWND, value: &str) {
    let utf16 = wide(value);
    let bytes = utf16.len() * 2;
    let Ok(memory) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }) else {
        return;
    };
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        unsafe {
            let _ = GlobalFree(Some(memory));
        }
        return;
    }
    unsafe {
        ptr::copy_nonoverlapping(utf16.as_ptr().cast::<u8>(), pointer.cast::<u8>(), bytes);
        let _ = GlobalUnlock(memory);
        if OpenClipboard(Some(hwnd)).is_err() {
            let _ = GlobalFree(Some(memory));
            return;
        }
        let _ = EmptyClipboard();
        if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))).is_err() {
            // The system owns the allocation only after SetClipboardData succeeds.
            let _ = CloseClipboard();
            let _ = GlobalFree(Some(memory));
            return;
        }
        let _ = CloseClipboard();
    }
}

/// Prepared off the UI thread; Windows takes ownership only after publication.
pub struct PreparedImage {
    memory: usize,
    _reservation: Option<crate::resources::Reservation>,
}
impl PreparedImage {
    pub fn new(width: u32, height: u32, pixels: &[u8]) -> Result<Self, String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or("Dimensiones no válidas.")?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err("Dimensiones no válidas.".into());
        }
        let reservation = crate::resources::cpu_budget()
            .reserve(expected + 40)
            .map_err(str::to_string)?;
        let memory =
            unsafe { GlobalAlloc(GMEM_MOVEABLE, expected + 40) }.map_err(|e| e.to_string())?;
        let image = Self {
            memory: memory.0 as usize,
            _reservation: Some(reservation),
        };
        let data = unsafe { GlobalLock(memory) };
        if data.is_null() {
            return Err("No se pudo preparar el portapapeles.".into());
        }
        unsafe {
            let header = data.cast::<u8>();
            ptr::write_bytes(header, 0, 40);
            ptr::write_unaligned(header.cast::<u32>(), 40);
            ptr::write_unaligned(header.add(4).cast::<i32>(), width as i32);
            ptr::write_unaligned(header.add(8).cast::<i32>(), height as i32);
            ptr::write_unaligned(header.add(12).cast::<u16>(), 1);
            ptr::write_unaligned(header.add(14).cast::<u16>(), 32);
            ptr::write_unaligned(header.add(20).cast::<u32>(), pixels.len() as u32);
            let destination = std::slice::from_raw_parts_mut(header.add(40), pixels.len());
            let row = width as usize * 4;
            for y in 0..height as usize {
                destination[(height as usize - 1 - y) * row..(height as usize - y) * row]
                    .copy_from_slice(&pixels[y * row..(y + 1) * row]);
            }
            let _ = GlobalUnlock(memory);
        }
        Ok(image)
    }
    pub fn publish(mut self, hwnd: HWND) -> Result<(), String> {
        unsafe {
            OpenClipboard(Some(hwnd)).map_err(|e| e.to_string())?;
            let result = EmptyClipboard().and_then(|()| {
                SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(self.memory as *mut _)))
            });
            if result.is_ok() {
                self.memory = 0;
            }
            let _ = CloseClipboard();
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }
}
impl Drop for PreparedImage {
    fn drop(&mut self) {
        if self.memory != 0 {
            unsafe {
                let _ = GlobalFree(Some(windows::Win32::Foundation::HGLOBAL(
                    self.memory as *mut _,
                )));
            }
        }
    }
}
