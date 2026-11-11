use std::{
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

use windows::{
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Graphics::Imaging::{
            CLSID_WICImagingFactory2, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
            GUID_WICPixelFormat32bppPBGRA, IWICBitmapEncoderInfo, IWICBitmapFrameDecode,
            IWICBitmapSource, IWICColorContext, IWICImagingFactory, IWICMetadataQueryReader,
            IWICMetadataQueryWriter, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
            WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand, WICEncoder, WICRect,
        },
        System::{
            Com::StructuredStorage::{PROPVARIANT, PropVariantClear},
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Variant::{VT_BSTR, VT_I2, VT_LPSTR, VT_LPWSTR, VT_UI2, VT_UI4},
        },
    },
    core::{Interface, PCWSTR, w},
};

use windows::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

const MAX_DECODE_BYTES: u64 = crate::resources::CPU_LIMIT as u64;
const MAX_FRAMES: u32 = 512;
const MAX_PREFETCH_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREFETCH_DECODE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREFETCH_SVG_BYTES: u64 = 512 * 1024;
const MAX_COPY_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIC_COPY_CHUNK_BYTES: usize = 16 * 1024 * 1024;
const MAX_SVG_BYTES: u64 = 64 * 1024 * 1024;
mod decoding;
mod encoding;
mod jxl;
mod metadata;
mod raster;
mod svg;
use decoding::color_managed_source;
use encoding::{convert_encoder_source, valid_resolution};
use jxl::decode_jxl;
use metadata::{read_metadata_string, read_metadata_u32, write_editable_metadata};
use raster::{
    clear_rect_cancellable, raster_document, render_raster_region, render_raster_view,
    source_region,
};
use svg::decode_svg;
mod export;
mod gif_source;
#[cfg(test)]
mod regression_tests;
mod request;
mod view;
mod wic_view;
use crate::resources::PixelBuffer;
use gif_source::decode_gif;
pub use request::DecodeRequest;
pub use view::EditOperation;
use view::{EditHistory, ViewTransform};
use wic_view::prepare_wic_view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentId(u64);
impl DocumentId {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Debug)]
pub struct ImageSource {
    path: PathBuf,
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}
impl ImageSource {
    fn open(path: &Path) -> ImageResult<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
    fn verify(&self) -> ImageResult<()> {
        let metadata = fs::metadata(&self.path)?;
        if metadata.len() != self.bytes || metadata.modified().ok() != self.modified {
            return Err(ImageError("El archivo cambió. Recárgalo antes de continuar. / The file changed. Reload it before continuing.".into()));
        }
        Ok(())
    }
}

pub trait SourceLease: std::fmt::Debug + Send + Sync {}
impl<T: std::fmt::Debug + Send + Sync> SourceLease for T {}
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct ImageError(pub String);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}
impl From<&'static str> for ImageError {
    fn from(value: &'static str) -> Self {
        Self(value.into())
    }
}

impl From<windows::core::Error> for ImageError {
    fn from(value: windows::core::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<std::io::Error> for ImageError {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

pub type ImageResult<T> = Result<T, ImageError>;

#[derive(Clone, Debug)]
pub struct ExportFormat {
    pub container: windows::core::GUID,
    pub name: String,
    pub extensions: Vec<String>,
}

impl ExportFormat {
    pub fn extension(&self) -> &str {
        self.extensions.first().map(String::as_str).unwrap_or("png")
    }

    pub fn filter_pattern(&self) -> String {
        self.extensions
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn filter_label(&self) -> String {
        format!("{} ({})", self.name, self.filter_pattern())
    }

    pub fn accepts_extension(&self, extension: &str) -> bool {
        self.extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension))
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: PixelBuffer,
    pub delay_ms: u32,
}

impl Frame {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Clone, Debug)]
pub struct ImageDocument {
    pub id: DocumentId,
    pub revision: u64,
    pub path: PathBuf,
    pub frames: Vec<Frame>,
    pub frame_index: usize,
    pub original_size: (u32, u32),
    pub resolution_dpi: (f64, f64),
    pub file_size: u64,
    pub format_name: String,
    pub metadata: Vec<(String, String)>,
    pub metadata_dirty: bool,
    pub dirty: bool,
    pub is_preview: bool,
    pub rendered_region: (u32, u32, u32, u32),
    pub source_frame_count: usize,
    pub source_lease: Option<Arc<dyn SourceLease>>,
    pub source: Option<ImageSource>,
    edits: EditHistory,
}

impl ImageDocument {
    pub fn current_frame(&self) -> &Frame {
        &self.frames[0]
    }

    pub fn frame_count(&self) -> usize {
        self.source_frame_count
    }

    pub fn view_dimensions(&self) -> (u32, u32) {
        let view = ViewTransform::from_operations(self.original_size, &self.edits.operations);
        (view.width, view.height)
    }

    pub fn apply_edit(&mut self, operation: EditOperation) {
        self.edits.push(operation);
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let changed = self.edits.undo();
        if changed {
            self.revision = self.revision.wrapping_add(1);
            self.dirty = true;
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        let changed = self.edits.redo();
        if changed {
            self.revision = self.revision.wrapping_add(1);
            self.dirty = true;
        }
        changed
    }

    pub fn rotate_right(&mut self) {
        self.apply_edit(EditOperation::RotateRight);
    }
    pub fn rotate_left(&mut self) {
        self.apply_edit(EditOperation::RotateLeft);
    }
    pub fn flip_horizontal(&mut self) {
        self.apply_edit(EditOperation::FlipHorizontal);
    }
    pub fn flip_vertical(&mut self) {
        self.apply_edit(EditOperation::FlipVertical);
    }
    pub fn crop(&mut self, l: u32, t: u32, r: u32, b: u32) -> bool {
        let (w, h) = self.view_dimensions();
        if r.min(w) <= l.min(w) || b.min(h) <= t.min(h) {
            return false;
        }
        self.apply_edit(EditOperation::Crop(l, t, r, b));
        true
    }

    pub fn render_view(
        &self,
        mut request: DecodeRequest,
        cancel: &impl Fn() -> bool,
        factory: Option<&IWICImagingFactory>,
    ) -> ImageResult<Self> {
        if let Some(source) = &self.source {
            source.verify()?;
        }
        request.operations = self.edits.operations.clone();
        let mut rendered = Self::decode_request(&self.path, cancel, factory, &request)?;
        rendered.id = self.id;
        rendered.revision = self.revision;
        rendered.metadata = self.metadata.clone();
        rendered.metadata_dirty = self.metadata_dirty;
        rendered.dirty = self.dirty;
        rendered.edits = self.edits.clone();
        rendered.source_lease = self.source_lease.clone();
        Ok(rendered)
    }

    #[cfg(test)]
    pub fn save_as(&mut self, path: &Path) -> ImageResult<()> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let container = match extension.as_str() {
            "jpg" | "jpeg" | "jpe" => GUID_ContainerFormatJpeg,
            "png" => windows::Win32::Graphics::Imaging::GUID_ContainerFormatPng,
            "bmp" | "dib" => windows::Win32::Graphics::Imaging::GUID_ContainerFormatBmp,
            "tif" | "tiff" => windows::Win32::Graphics::Imaging::GUID_ContainerFormatTiff,
            "gif" => windows::Win32::Graphics::Imaging::GUID_ContainerFormatGif,
            "wdp" | "jxr" | "hdp" => windows::Win32::Graphics::Imaging::GUID_ContainerFormatWmp,
            _ => {
                return Err(ImageError(
                    "Windows no tiene un codificador disponible para el formato elegido.".into(),
                ));
            }
        };
        self.save_as_container(path, container, &extension)
    }

    pub fn save_as_format(&mut self, path: &Path, format: &ExportFormat) -> ImageResult<()> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or(format.extension())
            .to_ascii_lowercase();
        self.save_as_container(path, format.container, &extension)
    }

    fn save_as_container(
        &mut self,
        path: &Path,
        container: windows::core::GUID,
        extension: &str,
    ) -> ImageResult<()> {
        self.write_container(path, container, extension, self.metadata_dirty)?;
        self.path = path.to_path_buf();
        self.original_size = self.view_dimensions();
        self.source_frame_count = 1;
        self.frame_index = 0;
        self.source_lease = None;
        self.source = Some(ImageSource::open(path)?);
        if self
            .edits
            .operations
            .iter()
            .filter(|op| matches!(op, EditOperation::RotateLeft | EditOperation::RotateRight))
            .count()
            % 2
            != 0
        {
            self.resolution_dpi = (self.resolution_dpi.1, self.resolution_dpi.0);
        }
        self.file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
        self.format_name = extension.to_ascii_uppercase();
        self.metadata_dirty = false;
        if !matches!(extension, "png" | "jpg" | "jpeg" | "jpe" | "tif" | "tiff") {
            self.metadata
                .retain(|(label, _)| label != "Autor" && label != "Descripción");
        }
        self.dirty = false;
        self.edits = EditHistory::default();
        Ok(())
    }

    pub fn export_copy_as(&self, path: &Path, format: &ExportFormat) -> ImageResult<()> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or(format.extension())
            .to_ascii_lowercase();
        self.write_container(path, format.container, &extension, false)
    }

    pub fn relocate_source(&mut self, path: PathBuf) {
        if let Some(source) = &mut self.source {
            source.path = path.clone();
        }
        self.path = path;
    }

    pub fn export_snapshot(&self) -> Self {
        self.clone()
    }
}

fn validate_dimensions(width: u32, height: u32) -> ImageResult<()> {
    if width == 0 || height == 0 || width > 100_000 || height > 100_000 {
        Err(ImageError(format!(
            "Dimensiones no válidas: {width} × {height}."
        )))
    } else {
        Ok(())
    }
}

pub fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests;
