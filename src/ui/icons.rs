//! Bitmap atlas generated at compile time from the user's original toolbar PNGs.
//! It is uploaded once to Direct2D and reused for every repaint.

use windows::Win32::Graphics::{
    Direct2D::{
        Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, ID2D1Bitmap,
        ID2D1HwndRenderTarget,
    },
    Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
};

use super::layout::PxRect;

const ICON_SIZE: u32 = 48;
const ICON_COUNT: u32 = 11;
const ATLAS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/toolbar-atlas.bgra"));

#[derive(Clone, Copy)]
pub enum Icon {
    Open = 0,
    Save = 1,
    Previous = 2,
    Next = 3,
    Info = 4,
    Copy = 5,
    CopyUrl = 6,
    RotateLeft = 7,
    RotateRight = 8,
    Fullscreen = 9,
    Download = 10,
}

pub unsafe fn create_atlas(target: &ID2D1HwndRenderTarget) -> windows::core::Result<ID2D1Bitmap> {
    let properties = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    unsafe {
        target.CreateBitmap(
            D2D_SIZE_U {
                width: ICON_SIZE * ICON_COUNT,
                height: ICON_SIZE,
            },
            Some(ATLAS.as_ptr().cast()),
            ICON_SIZE * ICON_COUNT * 4,
            &properties,
        )
    }
}

pub unsafe fn draw(
    target: &ID2D1HwndRenderTarget,
    icon: Icon,
    bounds: PxRect,
    atlas: &ID2D1Bitmap,
) {
    let available = (bounds.right - bounds.left).min(bounds.bottom - bounds.top);
    let size = (available - 5.0).max(16.0);
    let left = (bounds.left + bounds.right - size) * 0.5;
    let top = (bounds.top + bounds.bottom - size) * 0.5;
    let source_left = icon as u32 * ICON_SIZE;
    unsafe {
        target.DrawBitmap(
            atlas,
            Some(&D2D_RECT_F {
                left,
                top,
                right: left + size,
                bottom: top + size,
            }),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            Some(&D2D_RECT_F {
                left: source_left as f32,
                top: 0.0,
                right: (source_left + ICON_SIZE) as f32,
                bottom: ICON_SIZE as f32,
            }),
        )
    };
}
