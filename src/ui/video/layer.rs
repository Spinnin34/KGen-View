use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{
            COLORREF, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
            SIZE, WPARAM,
        },
        Graphics::{
            Direct2D::{
                Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
                D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1DCRenderTarget, ID2D1Factory,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::{
                AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
                CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
                HBITMAP, HDC, HGDIOBJ, SelectObject,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            Shell::{DragAcceptFiles, DragFinish, HDROP},
            WindowsAndMessaging::*,
        },
    },
    core::{Error, Result, w},
};

pub const WM_PANEL_INPUT: u32 = WM_APP + 13;
const CLASS: windows::core::PCWSTR = w!("KGenView.VideoControls");

pub struct Layer {
    pub hwnd: HWND,
    target: ID2D1DCRenderTarget,
    bitmap: Option<LayerBitmap>,
    pub visible: bool,
}

impl Layer {
    pub fn new(owner: HWND, factory: &ID2D1Factory) -> Result<Self> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                hInstance: instance,
                lpszClassName: CLASS,
                lpfnWndProc: Some(window_proc),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                let error = Error::from_thread();
                if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                    return Err(error);
                }
            }
            let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("KGen View Video Controls"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                Some(owner),
                None,
                Some(instance),
                None,
            )?;
            DragAcceptFiles(hwnd, true);
            Ok(Self {
                hwnd,
                target,
                bitmap: None,
                visible: false,
            })
        }
    }

    pub fn render(
        &mut self,
        origin: POINT,
        size: SIZE,
        draw: impl FnOnce(&ID2D1DCRenderTarget) -> Result<()>,
    ) -> Result<()> {
        if self
            .bitmap
            .as_ref()
            .is_none_or(|bitmap| bitmap.size != size)
        {
            let bitmap = LayerBitmap::new(size)?;
            unsafe {
                self.target.BindDC(
                    bitmap.dc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: size.cx,
                        bottom: size.cy,
                    },
                )?;
            }
            self.bitmap = Some(bitmap);
        }
        let bitmap = self.bitmap.as_ref().unwrap();
        unsafe {
            self.target.BeginDraw();
            self.target.Clear(None);
            let drawn = draw(&self.target);
            self.target.EndDraw(None, None)?;
            drawn?;
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&origin),
                Some(&size),
                Some(bitmap.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                }),
                ULW_ALPHA,
            )?;
            if !self.visible {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    pub fn reset(&mut self) {
        self.hide();
        self.bitmap = None;
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

struct LayerBitmap {
    size: SIZE,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl LayerBitmap {
    fn new(size: SIZE) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size.cx,
                    biHeight: -size.cy,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = std::ptr::null_mut();
            let bitmap =
                match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut pixels, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(error) => {
                        let _ = DeleteDC(dc);
                        return Err(error);
                    }
                };
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            Ok(Self {
                size,
                dc,
                bitmap,
                previous,
            })
        }
    }
}

impl Drop for LayerBitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if msg == WM_LBUTTONDOWN {
                    SetCapture(hwnd);
                }
                if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
                    let _ = PostMessageW(Some(owner), WM_PANEL_INPUT, WPARAM(msg as usize), lparam);
                }
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_DROPFILES => {
                let sent = GetWindow(hwnd, GW_OWNER)
                    .and_then(|owner| PostMessageW(Some(owner), msg, wparam, lparam));
                if sent.is_err() && msg == WM_DROPFILES {
                    DragFinish(HDROP(wparam.0 as *mut _));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
