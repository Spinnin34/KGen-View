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

pub fn copy_bgra_image(hwnd: HWND, width: u32, height: u32, pixels: &[u8]) {
    let expected = width as usize * height as usize * 4;
    if width == 0 || height == 0 || pixels.len() != expected {
        return;
    }
    let header_size = 40usize;
    let Ok(memory) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, header_size + pixels.len()) }) else {
        return;
    };
    let data = unsafe { GlobalLock(memory) };
    if data.is_null() {
        unsafe {
            let _ = GlobalFree(Some(memory));
        }
        return;
    }
    unsafe {
        let header = data.cast::<u8>();
        ptr::write_unaligned(header.cast::<u32>(), 40);
        ptr::write_unaligned(header.add(4).cast::<i32>(), width as i32);
        ptr::write_unaligned(header.add(8).cast::<i32>(), height as i32);
        ptr::write_unaligned(header.add(12).cast::<u16>(), 1);
        ptr::write_unaligned(header.add(14).cast::<u16>(), 32);
        ptr::write_unaligned(header.add(16).cast::<u32>(), 0);
        ptr::write_unaligned(header.add(20).cast::<u32>(), pixels.len() as u32);
        ptr::write_bytes(header.add(24), 0, 16);
        let destination = std::slice::from_raw_parts_mut(header.add(header_size), pixels.len());
        let row = width as usize * 4;
        for y in 0..height as usize {
            let src = y * row;
            let dst = (height as usize - 1 - y) * row;
            destination[dst..dst + row].copy_from_slice(&pixels[src..src + row]);
        }
        let _ = GlobalUnlock(memory);
        if OpenClipboard(Some(hwnd)).is_err() {
            let _ = GlobalFree(Some(memory));
            return;
        }
        let _ = EmptyClipboard();
        if SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(memory.0))).is_err() {
            let _ = CloseClipboard();
            let _ = GlobalFree(Some(memory));
            return;
        }
        let _ = CloseClipboard();
    }
}
