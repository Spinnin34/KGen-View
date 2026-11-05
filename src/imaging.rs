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

const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FRAMES: u32 = 512;
const MAX_PREFETCH_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREFETCH_DECODE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PREFETCH_SVG_BYTES: u64 = 512 * 1024;
const MAX_COPY_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIC_COPY_CHUNK_BYTES: usize = 16 * 1024 * 1024;
const MAX_SVG_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_BYTES: usize = 128 * 1024 * 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct ImageError(pub String);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}

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
    pub pixels: Arc<Vec<u8>>,
    pub delay_ms: u32,
}

impl Frame {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    frames: Vec<Frame>,
}

#[derive(Clone, Debug)]
pub struct ImageDocument {
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
    undo: Vec<DocumentSnapshot>,
    redo: Vec<DocumentSnapshot>,
}

impl ImageDocument {
    pub fn create_wic_factory() -> ImageResult<IWICImagingFactory> {
        unsafe {
            CoCreateInstance::<_, IWICImagingFactory>(
                &CLSID_WICImagingFactory2,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }
        .map_err(ImageError::from)
    }

    pub fn available_export_formats() -> ImageResult<Vec<ExportFormat>> {
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER)? };
        let encoders = unsafe { factory.CreateComponentEnumerator(WICEncoder.0 as u32, 0)? };
        let mut formats = Vec::new();

        loop {
            let mut entries = [None];
            let mut fetched = 0u32;
            let result = unsafe { encoders.Next(&mut entries, Some(&mut fetched)) };
            if result.is_err() || fetched == 0 {
                break;
            }
            let Some(entry) = entries[0].take() else {
                continue;
            };
            let Ok(info) = entry.cast::<IWICBitmapEncoderInfo>() else {
                continue;
            };

            let mut extension_buffer = [0u16; 512];
            let mut extension_length = 0u32;
            if unsafe { info.GetFileExtensions(&mut extension_buffer, &mut extension_length) }
                .is_err()
            {
                continue;
            }
            let extension_text = String::from_utf16_lossy(
                &extension_buffer[..(extension_length as usize).min(extension_buffer.len())],
            );
            let extension_text = extension_text.trim_end_matches('\0');
            let mut extensions: Vec<String> = extension_text
                .split([',', ';'])
                .filter_map(|value| {
                    let extension = value
                        .trim()
                        .trim_start_matches("*.")
                        .trim_start_matches('.')
                        .to_ascii_lowercase();
                    (!extension.is_empty()).then_some(extension)
                })
                .collect();
            extensions.sort_by_key(|extension| match extension.as_str() {
                "jpg" | "bmp" | "png" | "tif" | "gif" | "jxr" => 0,
                "jpeg" | "dib" | "tiff" | "wdp" => 1,
                "jpe" | "hdp" => 2,
                _ => 10,
            });
            extensions.dedup();
            if extensions.is_empty() {
                continue;
            }

            let mut name_buffer = [0u16; 128];
            let mut name_length = 0u32;
            let name =
                if unsafe { info.GetFriendlyName(&mut name_buffer, &mut name_length) }.is_ok() {
                    String::from_utf16_lossy(
                        &name_buffer[..(name_length as usize).min(name_buffer.len())],
                    )
                    .trim_end_matches('\0')
                    .trim()
                    .to_string()
                } else {
                    extensions[0].to_ascii_uppercase()
                };
            let container = match unsafe { info.GetContainerFormat() } {
                Ok(value) => value,
                Err(_) => continue,
            };
            if formats
                .iter()
                .any(|format: &ExportFormat| format.container == container)
            {
                continue;
            }
            formats.push(ExportFormat {
                container,
                name,
                extensions,
            });
        }

        formats.sort_by_key(|format| {
            let extension = format.extension();
            let rank = match extension {
                "png" => 0,
                "jpg" | "jpeg" => 1,
                "bmp" => 2,
                "tif" | "tiff" => 3,
                "gif" => 4,
                "jxr" | "wdp" | "hdp" => 5,
                _ => 10,
            };
            (rank, extension.to_string())
        });
        Ok(formats)
    }

    pub fn decode_prefetch_cancellable(
        path: &Path,
        should_cancel: &impl Fn() -> bool,
        factory: Option<&IWICImagingFactory>,
    ) -> Option<ImageResult<Self>> {
        if should_cancel() {
            return None;
        }
        let extension = path.extension().and_then(|value| value.to_str())?;
        let extension = extension.to_ascii_lowercase();
        if matches!(extension.as_str(), "gif" | "jxl") {
            return None;
        }
        let file_size = fs::metadata(path).ok()?.len();
        if file_size > MAX_PREFETCH_FILE_BYTES
            || (extension == "svg" && file_size > MAX_PREFETCH_SVG_BYTES)
        {
            return None;
        }
        Some(Self::decode_cancellable_with_limit(
            path,
            should_cancel,
            MAX_PREFETCH_DECODE_BYTES,
            factory,
        ))
    }

    #[cfg(test)]
    pub fn decode(path: &Path) -> ImageResult<Self> {
        Self::decode_cancellable(path, &|| false)
    }

    pub fn decode_cancellable(path: &Path, should_cancel: &impl Fn() -> bool) -> ImageResult<Self> {
        Self::decode_cancellable_with_limit(path, should_cancel, MAX_DECODE_BYTES, None)
    }

    pub fn decode_cancellable_with_factory(
        path: &Path,
        should_cancel: &impl Fn() -> bool,
        factory: &IWICImagingFactory,
    ) -> ImageResult<Self> {
        Self::decode_cancellable_with_limit(path, should_cancel, MAX_DECODE_BYTES, Some(factory))
    }

    fn decode_cancellable_with_limit(
        path: &Path,
        should_cancel: &impl Fn() -> bool,
        max_decode_bytes: u64,
        wic_factory: Option<&IWICImagingFactory>,
    ) -> ImageResult<Self> {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("gif"))
        {
            return decode_gif(&path, should_cancel);
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("svg"))
        {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            return decode_svg(&path, should_cancel, max_decode_bytes);
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jxl"))
        {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            return decode_jxl(&path);
        }
        let factory = match wic_factory {
            Some(factory) => factory.clone(),
            None => Self::create_wic_factory()?,
        };
        let wide = wide_path(&path);
        let decoder = unsafe {
            factory.CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?
        };
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let has_exif_metadata = matches!(
            extension.as_str(),
            "jpg"
                | "jpeg"
                | "jpe"
                | "png"
                | "tif"
                | "tiff"
                | "webp"
                | "heic"
                | "heif"
                | "wdp"
                | "jxr"
                | "hdp"
        );
        let has_jpeg_app1_metadata = matches!(extension.as_str(), "jpg" | "jpeg" | "jpe");
        let frame_count = unsafe { decoder.GetFrameCount()? }.min(MAX_FRAMES);
        if frame_count == 0 {
            return Err(ImageError("El archivo no contiene imágenes.".into()));
        }

        let mut frames = Vec::with_capacity(frame_count as usize);
        let mut metadata = Vec::new();
        let mut total_bytes = 0u64;
        let mut resolution_dpi = (96.0, 96.0);
        for index in 0..frame_count {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let source_frame = unsafe { decoder.GetFrame(index)? };
            let mut width = 0;
            let mut height = 0;
            unsafe { source_frame.GetSize(&mut width, &mut height)? };
            validate_dimensions(width, height)?;
            let stride = width
                .checked_mul(4)
                .ok_or_else(|| ImageError("Dimensiones no válidas.".into()))?;
            let len = u64::from(stride) * u64::from(height);
            total_bytes = total_bytes
                .checked_add(len)
                .ok_or_else(|| ImageError("Imagen demasiado grande.".into()))?;
            if total_bytes > max_decode_bytes {
                return Err(ImageError(format!(
                    "La imagen necesita más de {} MiB para decodificarse.",
                    max_decode_bytes / 1024 / 1024
                )));
            }

            // Reuse the frame's metadata reader for its fields instead of
            // creating a new COM reader for every individual property.
            let metadata_reader = if has_exif_metadata {
                unsafe { source_frame.GetMetadataQueryReader().ok() }
            } else {
                None
            };
            if index == 0 {
                let mut dpi_x = 96.0;
                let mut dpi_y = 96.0;
                if unsafe { source_frame.GetResolution(&mut dpi_x, &mut dpi_y) }.is_ok()
                    && dpi_x.is_finite()
                    && dpi_y.is_finite()
                    && dpi_x > 0.0
                    && dpi_y > 0.0
                {
                    resolution_dpi = (dpi_x, dpi_y);
                }
            }
            if index == 0
                && let Some(reader) = metadata_reader.as_ref()
            {
                if has_jpeg_app1_metadata {
                    for (label, query) in [
                        ("Fabricante", w!("/app1/ifd/{ushort=271}")),
                        ("Cámara", w!("/app1/ifd/{ushort=272}")),
                        ("Software", w!("/app1/ifd/{ushort=305}")),
                        ("Fecha original", w!("/app1/ifd/exif/{ushort=36867}")),
                        ("Autor", w!("/app1/ifd/{ushort=315}")),
                        ("Descripción", w!("/app1/ifd/{ushort=270}")),
                    ] {
                        if let Some(value) = read_metadata_string(reader, query) {
                            metadata.push((label.into(), value));
                        }
                    }
                }
                for (label, query) in [
                    ("Fabricante", w!("/ifd/{ushort=271}")),
                    ("Cámara", w!("/ifd/{ushort=272}")),
                    ("Software", w!("/ifd/{ushort=305}")),
                    ("Fecha original", w!("/ifd/exif/{ushort=36867}")),
                    ("Autor", w!("/ifd/{ushort=315}")),
                    ("Descripción", w!("/ifd/{ushort=270}")),
                ] {
                    if !metadata.iter().any(|(current, _)| current == label)
                        && let Some(value) = read_metadata_string(reader, query)
                    {
                        metadata.push((label.into(), value));
                    }
                }
                if extension == "png" {
                    for (label, query) in [
                        ("Autor", w!("/tEXt/{str=Author}")),
                        ("Descripción", w!("/tEXt/{str=Description}")),
                    ] {
                        if !metadata.iter().any(|(current, _)| current == label)
                            && let Some(value) = read_metadata_string(reader, query)
                        {
                            metadata.push((label.into(), value));
                        }
                    }
                }
            }
            let source = color_managed_source(&factory, &source_frame)?;
            let mut pixels = vec![0u8; len as usize];
            copy_pixels_cancellable(&source, width, height, stride, &mut pixels, should_cancel)?;
            let orientation = metadata_reader
                .as_ref()
                .and_then(|reader| read_metadata_u32(reader, w!("/app1/ifd/{ushort=274}")))
                .or_else(|| {
                    metadata_reader
                        .as_ref()
                        .and_then(|reader| read_metadata_u32(reader, w!("/ifd/{ushort=274}")))
                })
                .unwrap_or(1);
            if index == 0 && matches!(orientation, 5..=8) {
                resolution_dpi = (resolution_dpi.1, resolution_dpi.0);
            }
            frames.push(apply_orientation(
                Frame {
                    width,
                    height,
                    pixels: Arc::new(pixels),
                    delay_ms: 100,
                },
                orientation,
                should_cancel,
            )?);
        }

        let file_size = fs::metadata(&path).map(|value| value.len()).unwrap_or(0);
        let original_size = frames
            .first()
            .map(|frame| (frame.width, frame.height))
            .unwrap_or((0, 0));
        let format_name = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("?")
            .to_ascii_uppercase();
        Ok(Self {
            path,
            frames,
            frame_index: 0,
            original_size,
            resolution_dpi,
            file_size,
            format_name,
            metadata,
            metadata_dirty: false,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn current_frame(&self) -> &Frame {
        &self.frames[self.frame_index.min(self.frames.len() - 1)]
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn next_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frame_index = (self.frame_index + 1) % self.frames.len();
        }
    }

    pub fn previous_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frame_index = (self.frame_index + self.frames.len() - 1) % self.frames.len();
        }
    }

    fn checkpoint(&mut self) {
        let bytes = self.frames.iter().fold(0usize, |total, frame| {
            total.saturating_add(frame.byte_len())
        });
        self.redo.clear();
        if bytes <= MAX_HISTORY_BYTES {
            while self.history_bytes().saturating_add(bytes) > MAX_HISTORY_BYTES {
                if self.undo.is_empty() {
                    break;
                }
                self.undo.remove(0);
            }
            self.undo.push(DocumentSnapshot {
                frames: self.frames.clone(),
            });
        } else {
            self.undo.clear();
        }
        self.dirty = true;
    }

    fn history_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .flat_map(|snapshot| snapshot.frames.iter())
            .fold(0usize, |total, frame| {
                total.saturating_add(frame.byte_len())
            })
    }

    fn trim_history(&mut self) {
        while self.history_bytes() > MAX_HISTORY_BYTES {
            if !self.undo.is_empty() {
                self.undo.remove(0);
            } else if !self.redo.is_empty() {
                self.redo.remove(0);
            } else {
                break;
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(DocumentSnapshot {
            frames: std::mem::replace(&mut self.frames, snapshot.frames),
        });
        self.trim_history();
        self.frame_index = self.frame_index.min(self.frames.len().saturating_sub(1));
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(DocumentSnapshot {
            frames: std::mem::replace(&mut self.frames, snapshot.frames),
        });
        self.trim_history();
        self.frame_index = self.frame_index.min(self.frames.len().saturating_sub(1));
        self.dirty = true;
        true
    }

    pub fn rotate_right(&mut self) {
        self.checkpoint();
        for frame in &mut self.frames {
            *frame = rotate(frame, true);
        }
    }

    pub fn rotate_left(&mut self) {
        self.checkpoint();
        for frame in &mut self.frames {
            *frame = rotate(frame, false);
        }
    }

    pub fn flip_horizontal(&mut self) {
        self.checkpoint();
        for frame in &mut self.frames {
            flip_pixels_horizontal(
                Arc::make_mut(&mut frame.pixels).as_mut_slice(),
                frame.width as usize,
                frame.height as usize,
            );
        }
    }

    pub fn flip_vertical(&mut self) {
        self.checkpoint();
        for frame in &mut self.frames {
            flip_pixels_vertical(
                Arc::make_mut(&mut frame.pixels).as_mut_slice(),
                frame.width as usize,
                frame.height as usize,
            );
        }
    }

    pub fn crop(&mut self, left: u32, top: u32, right: u32, bottom: u32) -> bool {
        let frame = self.current_frame();
        let left = left.min(frame.width);
        let top = top.min(frame.height);
        let right = right.min(frame.width);
        let bottom = bottom.min(frame.height);
        if right <= left || bottom <= top {
            return false;
        }
        self.checkpoint();
        for frame in &mut self.frames {
            let l = left.min(frame.width);
            let t = top.min(frame.height);
            let r = right.min(frame.width);
            let b = bottom.min(frame.height);
            let width = r - l;
            let height = b - t;
            let mut pixels = vec![0; (width * height * 4) as usize];
            for y in 0..height {
                let src = (((t + y) * frame.width + l) * 4) as usize;
                let dst = (y * width * 4) as usize;
                let count = (width * 4) as usize;
                pixels[dst..dst + count].copy_from_slice(&frame.pixels[src..src + count]);
            }
            frame.width = width;
            frame.height = height;
            frame.pixels = Arc::new(pixels);
        }
        true
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
        let frame = self.current_frame();
        self.original_size = (frame.width, frame.height);
        self.file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
        self.format_name = extension.to_ascii_uppercase();
        self.metadata_dirty = false;
        if !matches!(extension, "png" | "jpg" | "jpeg" | "jpe" | "tif" | "tiff") {
            self.metadata
                .retain(|(label, _)| label != "Autor" && label != "Descripción");
        }
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
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

    pub fn export_snapshot(&self) -> Self {
        Self {
            path: self.path.clone(),
            frames: vec![self.current_frame().clone()],
            frame_index: 0,
            original_size: self.original_size,
            resolution_dpi: self.resolution_dpi,
            file_size: self.file_size,
            format_name: self.format_name.clone(),
            metadata: self.metadata.clone(),
            metadata_dirty: false,
            dirty: self.dirty,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn write_container(
        &self,
        path: &Path,
        container: windows::core::GUID,
        extension: &str,
        metadata_dirty: bool,
    ) -> ImageResult<()> {
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER)? };
        let temp_path = create_temp_sibling(path)?;
        let result =
            self.encode_to_path(&factory, &temp_path, container, extension, metadata_dirty);
        if let Err(error) = result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        let temp_wide = wide_path(&temp_path);
        let target_wide = wide_path(path);
        if let Err(error) = unsafe {
            MoveFileExW(
                PCWSTR(temp_wide.as_ptr()),
                PCWSTR(target_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    fn encode_to_path(
        &self,
        factory: &IWICImagingFactory,
        path: &Path,
        container: windows::core::GUID,
        extension: &str,
        metadata_dirty: bool,
    ) -> ImageResult<()> {
        if container == windows::Win32::Graphics::Imaging::GUID_ContainerFormatGif {
            return self.encode_gif_to_path(path);
        }
        let stream = unsafe { factory.CreateStream()? };
        let wide = wide_path(path);
        unsafe { stream.InitializeFromFilename(PCWSTR(wide.as_ptr()), GENERIC_WRITE.0)? };
        let encoder = unsafe { factory.CreateEncoder(&container, std::ptr::null())? };
        unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache)? };
        let frame = self.current_frame();
        let mut encoded = None;
        let mut options = None;
        unsafe {
            encoder.CreateNewFrame(&mut encoded, &mut options)?;
            let encoded =
                encoded.ok_or_else(|| ImageError("No se pudo crear el codificador.".into()))?;
            encoded.Initialize(options.as_ref())?;
            encoded.SetSize(frame.width, frame.height)?;
            let (dpi_x, dpi_y) = valid_resolution(self.resolution_dpi);
            encoded.SetResolution(dpi_x, dpi_y)?;
            let mut pixel_format = GUID_WICPixelFormat32bppPBGRA;
            encoded.SetPixelFormat(&mut pixel_format)?;
            let source = if container == GUID_ContainerFormatJpeg {
                let stride = frame
                    .width
                    .checked_mul(3)
                    .ok_or_else(|| ImageError("Dimensiones no válidas para JPEG.".into()))?;
                let mut pixels = Vec::with_capacity(stride as usize * frame.height as usize);
                for bgra in frame.pixels.as_chunks::<4>().0 {
                    let alpha = u16::from(bgra[3]);
                    let background = 255u16 - alpha;
                    pixels.extend_from_slice(&[
                        (u16::from(bgra[0]) + background).min(255) as u8,
                        (u16::from(bgra[1]) + background).min(255) as u8,
                        (u16::from(bgra[2]) + background).min(255) as u8,
                    ]);
                }
                let bitmap = factory.CreateBitmapFromMemory(
                    frame.width,
                    frame.height,
                    &GUID_WICPixelFormat24bppBGR,
                    stride,
                    &pixels,
                )?;
                let source: IWICBitmapSource = bitmap.cast()?;
                convert_encoder_source(
                    factory,
                    &source,
                    &GUID_WICPixelFormat24bppBGR,
                    &pixel_format,
                )?
            } else {
                let bitmap = factory.CreateBitmapFromMemory(
                    frame.width,
                    frame.height,
                    &GUID_WICPixelFormat32bppPBGRA,
                    frame.width * 4,
                    &frame.pixels,
                )?;
                let source: IWICBitmapSource = bitmap.cast()?;
                convert_encoder_source(
                    factory,
                    &source,
                    &GUID_WICPixelFormat32bppPBGRA,
                    &pixel_format,
                )?
            };
            encoded.WriteSource(&source, std::ptr::null())?;
            write_editable_metadata(
                &encoded.GetMetadataQueryWriter(),
                extension,
                &self.metadata,
                metadata_dirty,
            )?;
            encoded.Commit()?;
            encoder.Commit()?;
        }
        Ok(())
    }

    fn encode_gif_to_path(&self, path: &Path) -> ImageResult<()> {
        let frame = self.current_frame();
        let width = u16::try_from(frame.width)
            .map_err(|_| ImageError("El ancho supera el límite del formato GIF.".into()))?;
        let height = u16::try_from(frame.height)
            .map_err(|_| ImageError("El alto supera el límite del formato GIF.".into()))?;
        let mut rgba = Vec::with_capacity(frame.pixels.len());
        for bgra in frame.pixels.as_chunks::<4>().0 {
            let alpha = u16::from(bgra[3]);
            let unpremultiply = |channel: u8| {
                (u16::from(channel) * 255 + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8
            };
            rgba.extend_from_slice(&[
                unpremultiply(bgra[2]),
                unpremultiply(bgra[1]),
                unpremultiply(bgra[0]),
                if alpha < 128 { 0 } else { 255 },
            ]);
        }
        let output = fs::File::create(path)?;
        let mut encoder = gif::Encoder::new(output, width, height, &[])
            .map_err(|error| ImageError(format!("No se pudo crear el archivo GIF: {error}")))?;
        let encoded = gif::Frame::from_rgba_speed(width, height, &mut rgba, 10);
        encoder
            .write_frame(&encoded)
            .map_err(|error| ImageError(format!("No se pudo codificar el fotograma GIF: {error}")))
    }
}

fn convert_encoder_source(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    source_format: &windows::core::GUID,
    target_format: &windows::core::GUID,
) -> ImageResult<IWICBitmapSource> {
    if source_format == target_format {
        return Ok(source.clone());
    }
    let converter = unsafe { factory.CreateFormatConverter()? };
    unsafe {
        converter.Initialize(
            source,
            target_format,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
    }
    Ok(converter.cast()?)
}

fn create_temp_sibling(path: &Path) -> ImageResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..32 {
        let id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".kgen-view-{}-{id}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(ImageError(
        "No se pudo crear un archivo temporal para guardar.".into(),
    ))
}

fn valid_resolution((dpi_x, dpi_y): (f64, f64)) -> (f64, f64) {
    if dpi_x.is_finite() && dpi_y.is_finite() && dpi_x > 0.0 && dpi_y > 0.0 {
        (dpi_x, dpi_y)
    } else {
        (96.0, 96.0)
    }
}

fn write_editable_metadata(
    writer: &windows::core::Result<IWICMetadataQueryWriter>,
    extension: &str,
    metadata: &[(String, String)],
    metadata_dirty: bool,
) -> ImageResult<()> {
    let author = metadata
        .iter()
        .find(|(label, _)| label == "Autor")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    let description = metadata
        .iter()
        .find(|(label, _)| label == "Descripción")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    if author.is_empty() && description.is_empty() {
        return Ok(());
    }

    let queries = match extension {
        "jpg" | "jpeg" | "jpe" => [
            ("Autor", w!("/app1/ifd/{ushort=315}")),
            ("Descripción", w!("/app1/ifd/{ushort=270}")),
        ],
        "tif" | "tiff" => [
            ("Autor", w!("/ifd/{ushort=315}")),
            ("Descripción", w!("/ifd/{ushort=270}")),
        ],
        "png" => [
            ("Autor", w!("/tEXt/{str=Author}")),
            ("Descripción", w!("/tEXt/{str=Description}")),
        ],
        _ => {
            if !metadata_dirty {
                return Ok(());
            }
            return Err(ImageError(
                "El formato elegido no admite guardar autor y descripción. Usa PNG, JPEG o TIFF."
                    .into(),
            ));
        }
    };
    let writer = writer
        .as_ref()
        .map_err(|error| ImageError(error.to_string()))?;
    for (label, query) in queries {
        let value = if label == "Autor" {
            author
        } else {
            description
        };
        if !value.is_empty() {
            write_metadata_string(writer, query, value)?;
        }
    }
    Ok(())
}

fn write_metadata_string(
    writer: &IWICMetadataQueryWriter,
    query: PCWSTR,
    value: &str,
) -> ImageResult<()> {
    let mut wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut variant = PROPVARIANT::default();
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = windows::core::PWSTR(wide.as_mut_ptr());
        writer.SetMetadataByName(query, &variant)?;
    }
    Ok(())
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

fn color_managed_source(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> ImageResult<IWICBitmapSource> {
    let mut count = 0u32;
    let mut contexts: Vec<Option<IWICColorContext>> = Vec::new();
    // WIC reports the required context count even when the first call has no
    // output slots. That sizing call commonly returns INSUFFICIENTBUFFER, so
    // its HRESULT must not decide whether the source has an ICC profile.
    let _ = unsafe { frame.GetColorContexts(&mut contexts, &mut count) };
    let has_profile = (1..=16).contains(&count);
    if has_profile {
        contexts.resize_with(count as usize, || None);
        if unsafe { frame.GetColorContexts(&mut contexts, &mut count) }.is_ok()
            && let Some(source_context) = contexts.into_iter().flatten().next()
        {
            let destination = unsafe { factory.CreateColorContext()? };
            if unsafe { destination.InitializeFromExifColorSpace(1) }.is_ok() {
                let transform = unsafe { factory.CreateColorTransformer()? };
                if unsafe {
                    transform.Initialize(
                        frame,
                        &source_context,
                        &destination,
                        &GUID_WICPixelFormat32bppPBGRA,
                    )
                }
                .is_ok()
                {
                    return Ok(transform.cast()?);
                }
            }
        }
    }
    let converter = unsafe { factory.CreateFormatConverter()? };
    unsafe {
        converter.Initialize(
            frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
    }
    Ok(converter.cast()?)
}

fn copy_pixels_cancellable(
    source: &IWICBitmapSource,
    width: u32,
    height: u32,
    stride: u32,
    pixels: &mut [u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let stride = stride as usize;
    let rows_per_chunk = (MAX_WIC_COPY_CHUNK_BYTES / stride)
        .max(1)
        .min(height as usize);
    for top in (0..height as usize).step_by(rows_per_chunk) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let rows = rows_per_chunk.min(height as usize - top);
        let rect = WICRect {
            X: 0,
            Y: top as i32,
            Width: width as i32,
            Height: rows as i32,
        };
        let start = top * stride;
        let end = start + rows * stride;
        if unsafe { source.CopyPixels(&rect, stride as u32, &mut pixels[start..end]) }.is_err() {
            // Some installed WIC codecs only support full-frame copies. Keep
            // those formats working and still stop before the fallback if the
            // user has already replaced this request.
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            unsafe { source.CopyPixels(std::ptr::null(), stride as u32, pixels)? };
            return Ok(());
        }
    }
    Ok(())
}

fn read_metadata_u32(reader: &IWICMetadataQueryReader, query: PCWSTR) -> Option<u32> {
    let mut value = PROPVARIANT::default();
    unsafe { reader.GetMetadataByName(query, &mut value) }.ok()?;
    let result = unsafe {
        let inner = &*value.Anonymous.Anonymous;
        if inner.vt == VT_UI2 {
            Some(inner.Anonymous.uiVal as u32)
        } else if inner.vt == VT_UI4 {
            Some(inner.Anonymous.ulVal)
        } else if inner.vt == VT_I2 {
            Some(inner.Anonymous.iVal.max(0) as u32)
        } else {
            None
        }
    };
    unsafe {
        let _ = PropVariantClear(&mut value);
    }
    result
}

fn read_metadata_string(reader: &IWICMetadataQueryReader, query: PCWSTR) -> Option<String> {
    let mut value = PROPVARIANT::default();
    unsafe { reader.GetMetadataByName(query, &mut value) }.ok()?;
    let result = unsafe {
        let inner = &*value.Anonymous.Anonymous;
        if inner.vt == VT_LPWSTR {
            inner.Anonymous.pwszVal.to_string().ok()
        } else if inner.vt == VT_LPSTR {
            inner.Anonymous.pszVal.to_string().ok()
        } else if inner.vt == VT_BSTR {
            Some(inner.Anonymous.bstrVal.to_string())
        } else if inner.vt == VT_UI2 {
            Some(inner.Anonymous.uiVal.to_string())
        } else if inner.vt == VT_UI4 {
            Some(inner.Anonymous.ulVal.to_string())
        } else {
            None
        }
    };
    unsafe {
        let _ = PropVariantClear(&mut value);
    }
    result.filter(|text| !text.trim().is_empty())
}

fn apply_orientation(
    mut frame: Frame,
    orientation: u32,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    match orientation {
        2 => flip_pixels_horizontal_cancellable(
            Arc::make_mut(&mut frame.pixels).as_mut_slice(),
            frame.width as usize,
            frame.height as usize,
            should_cancel,
        )?,
        3 => reverse_pixels_180_cancellable(
            Arc::make_mut(&mut frame.pixels).as_mut_slice(),
            should_cancel,
        )?,
        4 => flip_pixels_vertical_cancellable(
            Arc::make_mut(&mut frame.pixels).as_mut_slice(),
            frame.width as usize,
            frame.height as usize,
            should_cancel,
        )?,
        5..=8 => return transform_frame_cancellable(&frame, orientation, should_cancel),
        _ => {}
    }
    Ok(frame)
}

fn reverse_pixels_180_cancellable(
    pixels: &mut [u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let pixel_count = pixels.len() / 4;
    for index in 0..pixel_count / 2 {
        if index % 65_536 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let left = index * 4;
        let right = (pixel_count - 1 - index) * 4;
        let (before_right, right_pixel) = pixels.split_at_mut(right);
        before_right[left..left + 4].swap_with_slice(&mut right_pixel[..4]);
    }
    Ok(())
}

fn flip_pixels_horizontal_cancellable(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let row_len = width * 4;
    for y in 0..height {
        if y % 32 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let row_start = y * row_len;
        let row = &mut pixels[row_start..row_start + row_len];
        for x in 0..width / 2 {
            let left_idx = x * 4;
            let right_idx = (width - 1 - x) * 4;
            for c in 0..4 {
                row.swap(left_idx + c, right_idx + c);
            }
        }
    }
    Ok(())
}

fn flip_pixels_horizontal(pixels: &mut [u8], width: usize, height: usize) {
    flip_pixels_horizontal_cancellable(pixels, width, height, &|| false)
        .expect("uncancellable horizontal flip");
}

fn flip_pixels_vertical_cancellable(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let row_len = width * 4;
    for y in 0..height / 2 {
        if y % 32 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let other = height - 1 - y;
        let top_start = y * row_len;
        let bottom_start = other * row_len;
        let (top_half, bottom_half) = pixels.split_at_mut(bottom_start);
        top_half[top_start..top_start + row_len].swap_with_slice(&mut bottom_half[..row_len]);
    }
    Ok(())
}

fn flip_pixels_vertical(pixels: &mut [u8], width: usize, height: usize) {
    flip_pixels_vertical_cancellable(pixels, width, height, &|| false)
        .expect("uncancellable vertical flip");
}

fn transform_frame(frame: &Frame, orientation: u32) -> Frame {
    transform_frame_cancellable(frame, orientation, &|| false)
        .expect("uncancellable frame transform")
}

fn transform_frame_cancellable(
    frame: &Frame,
    orientation: u32,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let swaps_dimensions = matches!(orientation, 5..=8);
    let (new_width, new_height) = if swaps_dimensions {
        (height, width)
    } else {
        (width, height)
    };
    let mut pixels = vec![0; frame.pixels.len()];

    // Tiling keeps both source reads and rotated destination writes near each
    // other in memory. A single pass also avoids the intermediate full-size
    // frames previously allocated for compound EXIF orientations.
    const TILE: usize = 32;
    let reverse_y = matches!(orientation, 6 | 7);
    for tile_x in (0..width).step_by(TILE) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let x_end = (tile_x + TILE).min(width);
        for tile_y in (0..height).step_by(TILE) {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let y_end = (tile_y + TILE).min(height);
            for x in tile_x..x_end {
                if reverse_y {
                    for y in (tile_y..y_end).rev() {
                        copy_oriented_pixel(
                            frame,
                            &mut pixels,
                            width,
                            new_width,
                            x,
                            y,
                            orientation,
                        );
                    }
                } else {
                    for y in tile_y..y_end {
                        copy_oriented_pixel(
                            frame,
                            &mut pixels,
                            width,
                            new_width,
                            x,
                            y,
                            orientation,
                        );
                    }
                }
            }
        }
    }

    Ok(Frame {
        width: new_width as u32,
        height: new_height as u32,
        pixels: Arc::new(pixels),
        delay_ms: frame.delay_ms,
    })
}

fn copy_oriented_pixel(
    source: &Frame,
    destination: &mut [u8],
    source_width: usize,
    destination_width: usize,
    x: usize,
    y: usize,
    orientation: u32,
) {
    let source_offset = (y * source_width + x) * 4;
    let (target_x, target_y) = match orientation {
        5 => (y, x),
        6 => (source.height as usize - 1 - y, x),
        7 => (source.height as usize - 1 - y, source_width - 1 - x),
        8 => (y, source_width - 1 - x),
        _ => unreachable!("only axis-swapping orientations use this path"),
    };
    let destination_offset = (target_y * destination_width + target_x) * 4;
    destination[destination_offset..destination_offset + 4]
        .copy_from_slice(&source.pixels[source_offset..source_offset + 4]);
}

fn decode_gif(path: &Path, should_cancel: &impl Fn() -> bool) -> ImageResult<ImageDocument> {
    use gif::{ColorOutput, DecodeOptions, DisposalMethod};

    let file = fs::File::open(path)?;
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    let mut decoder = options
        .read_info(file)
        .map_err(|error| ImageError(error.to_string()))?;
    let width = u32::from(decoder.width());
    let height = u32::from(decoder.height());
    validate_dimensions(width, height)?;
    let canvas_len = u64::from(width) * u64::from(height) * 4;
    if canvas_len > MAX_DECODE_BYTES {
        return Err(ImageError("GIF demasiado grande.".into()));
    }
    let mut canvas = vec![0u8; canvas_len as usize];
    let mut frames = Vec::new();
    let mut previous_disposal = DisposalMethod::Keep;
    let mut previous_rect = (0u32, 0u32, 0u32, 0u32);
    let mut restore_canvas: Option<Vec<u8>> = None;

    while frames.len() < MAX_FRAMES as usize {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let Some(frame) = decoder
            .read_next_frame()
            .map_err(|error| ImageError(error.to_string()))?
        else {
            break;
        };
        match previous_disposal {
            DisposalMethod::Background => {
                clear_rect_cancellable(&mut canvas, width, previous_rect, should_cancel)?
            }
            DisposalMethod::Previous => {
                if let Some(previous) = restore_canvas.take() {
                    canvas = previous;
                }
            }
            _ => {}
        }
        let left = u32::from(frame.left);
        let top = u32::from(frame.top);
        let frame_width = u32::from(frame.width);
        let frame_height = u32::from(frame.height);
        if left + frame_width > width || top + frame_height > height {
            return Err(ImageError("Fotograma GIF fuera del lienzo lógico.".into()));
        }
        // Account for the decoded frame buffer, output frame, working canvas,
        // and a possible restore-to-previous snapshot before cloning them.
        let working_copies = if frame.dispose == DisposalMethod::Previous {
            4u64
        } else {
            3u64
        };
        let projected_bytes = (frames.len() as u64)
            .saturating_add(working_copies)
            .saturating_mul(canvas_len);
        if projected_bytes > MAX_DECODE_BYTES {
            return Err(ImageError(
                "La animación GIF supera el límite de 512 MiB.".into(),
            ));
        }
        restore_canvas = if frame.dispose == DisposalMethod::Previous {
            Some(clone_bytes_cancellable(&canvas, should_cancel)?)
        } else {
            None
        };
        for y in 0..frame_height {
            if y % 32 == 0 && should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            for x in 0..frame_width {
                let source = ((y * frame_width + x) * 4) as usize;
                let target = (((top + y) * width + left + x) * 4) as usize;
                let alpha = frame.buffer[source + 3];
                if alpha != 0 {
                    let red = frame.buffer[source];
                    let green = frame.buffer[source + 1];
                    let blue = frame.buffer[source + 2];
                    canvas[target..target + 4].copy_from_slice(&[blue, green, red, alpha]);
                }
            }
        }
        let delay_ms = u32::from(frame.delay).saturating_mul(10).max(20);
        frames.push(Frame {
            width,
            height,
            pixels: Arc::new(clone_bytes_cancellable(&canvas, should_cancel)?),
            delay_ms,
        });
        previous_disposal = frame.dispose;
        previous_rect = (left, top, frame_width, frame_height);
    }
    if frames.is_empty() {
        return Err(ImageError("El GIF no contiene fotogramas.".into()));
    }
    let file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
    Ok(ImageDocument {
        path: path.to_path_buf(),
        original_size: (width, height),
        resolution_dpi: (96.0, 96.0),
        frames,
        frame_index: 0,
        file_size,
        format_name: "GIF".into(),
        metadata: Vec::new(),
        metadata_dirty: false,
        dirty: false,
        undo: Vec::new(),
        redo: Vec::new(),
    })
}

fn decode_svg(
    path: &Path,
    should_cancel: &impl Fn() -> bool,
    max_decode_bytes: u64,
) -> ImageResult<ImageDocument> {
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let mut file = fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    if file_size > MAX_SVG_BYTES {
        return Err(ImageError(
            "El archivo SVG supera el límite de 64 MiB.".into(),
        ));
    }
    let mut data = Vec::with_capacity(file_size as usize);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if data.len().saturating_add(read) as u64 > MAX_SVG_BYTES {
            return Err(ImageError(
                "El archivo SVG supera el límite de 64 MiB.".into(),
            ));
        }
        data.extend_from_slice(&chunk[..read]);
    }
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt)
        .map_err(|error| ImageError(format!("Error decoding SVG: {error}")))?;
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let size = tree.size();
    let width = (size.width().ceil() as u32).max(1);
    let height = (size.height().ceil() as u32).max(1);
    validate_dimensions(width, height)?;
    let pixel_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ImageError("Dimensiones SVG no válidas.".into()))?;
    if pixel_bytes > max_decode_bytes {
        return Err(ImageError(format!(
            "La imagen SVG supera el límite de {} MiB.",
            max_decode_bytes / 1024 / 1024
        )));
    }
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| ImageError("No se pudo crear mapa de píxeles para SVG.".into()))?;
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    for chunk in pixmap.data_mut().chunks_mut(MAX_COPY_CHUNK_BYTES) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        for pixel in chunk.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    let pixels = pixmap.take();

    Ok(ImageDocument {
        path: path.to_path_buf(),
        original_size: (width, height),
        resolution_dpi: (96.0, 96.0),
        frames: vec![Frame {
            width,
            height,
            pixels: Arc::new(pixels),
            delay_ms: 100,
        }],
        frame_index: 0,
        file_size,
        format_name: "SVG".into(),
        metadata: Vec::new(),
        metadata_dirty: false,
        dirty: false,
        undo: Vec::new(),
        redo: Vec::new(),
    })
}

fn decode_jxl(path: &Path) -> ImageResult<ImageDocument> {
    let file_size = fs::metadata(path)?.len();
    if file_size > MAX_DECODE_BYTES {
        return Err(ImageError(
            "El archivo JPEG XL supera el límite de 512 MiB.".into(),
        ));
    }

    let tracker = jxl_oxide::AllocTracker::with_limit(MAX_DECODE_BYTES as usize);
    let mut decoder = jxl_oxide::JxlImage::builder()
        .alloc_tracker(tracker)
        .open(path)
        .map_err(|error| ImageError(format!("No se pudo decodificar JPEG XL: {error}")))?;
    let width = decoder.width();
    let height = decoder.height();
    validate_dimensions(width, height)?;

    let pixel_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ImageError("Dimensiones JPEG XL no válidas.".into()))?;
    if pixel_bytes > MAX_DECODE_BYTES {
        return Err(ImageError(
            "La imagen JPEG XL supera el límite de 512 MiB.".into(),
        ));
    }

    decoder.set_cms(jxl_oxide::Moxcms);
    decoder.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
        jxl_oxide::RenderingIntent::Relative,
    ));
    let render = decoder
        .render_frame(0)
        .map_err(|error| ImageError(format!("No se pudo renderizar JPEG XL: {error}")))?;
    let mut stream = render.stream();
    let channels = stream.channels() as usize;
    if !(3..=4).contains(&channels) {
        return Err(ImageError(
            "JPEG XL produjo un formato de píxel no compatible.".into(),
        ));
    }
    let sample_count = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| ImageError("Dimensiones JPEG XL no válidas.".into()))?;
    let mut pixels = Vec::with_capacity(pixel_bytes as usize);
    pixels.resize(sample_count, 0);
    let written = stream.write_to_buffer(&mut pixels);
    if written != sample_count {
        return Err(ImageError(
            "No se pudieron leer los píxeles JPEG XL.".into(),
        ));
    }

    if channels == 4 {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let [red, green, blue, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
            let premultiply = |color: u8| ((u16::from(color) * u16::from(alpha) + 127) / 255) as u8;
            pixel.copy_from_slice(&[
                premultiply(blue),
                premultiply(green),
                premultiply(red),
                alpha,
            ]);
        }
    } else {
        let pixel_count = (width as usize) * (height as usize);
        pixels.resize(pixel_bytes as usize, 0);
        for index in (0..pixel_count).rev() {
            let source = index * 3;
            let target = index * 4;
            let red = pixels[source];
            let green = pixels[source + 1];
            let blue = pixels[source + 2];
            pixels[target..target + 4].copy_from_slice(&[blue, green, red, 255]);
        }
    }

    Ok(ImageDocument {
        path: path.to_path_buf(),
        original_size: (width, height),
        resolution_dpi: (96.0, 96.0),
        frames: vec![Frame {
            width,
            height,
            pixels: Arc::new(pixels),
            delay_ms: 100,
        }],
        frame_index: 0,
        file_size,
        format_name: "JPEG XL".into(),
        metadata: Vec::new(),
        metadata_dirty: false,
        dirty: false,
        undo: Vec::new(),
        redo: Vec::new(),
    })
}

fn clone_bytes_cancellable(
    source: &[u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Vec<u8>> {
    let mut copy = vec![0; source.len()];
    for start in (0..source.len()).step_by(MAX_COPY_CHUNK_BYTES) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let end = (start + MAX_COPY_CHUNK_BYTES).min(source.len());
        copy[start..end].copy_from_slice(&source[start..end]);
    }
    Ok(copy)
}

fn clear_rect_cancellable(
    canvas: &mut [u8],
    canvas_width: u32,
    rect: (u32, u32, u32, u32),
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let (left, top, width, height) = rect;
    for (row, y) in (top..top.saturating_add(height)).enumerate() {
        if row % 32 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let start = ((y * canvas_width + left) * 4) as usize;
        if start >= canvas.len() {
            break;
        }
        let end = start.saturating_add((width * 4) as usize).min(canvas.len());
        if start < end {
            canvas[start..end].fill(0);
        }
    }
    Ok(())
}

fn rotate(frame: &Frame, clockwise: bool) -> Frame {
    transform_frame(frame, if clockwise { 6 } else { 8 })
}

pub fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    #[test]
    fn decode_cancels_before_opening_a_stale_request() {
        let result = ImageDocument::decode_cancellable(
            Path::new("stale-navigation-request-that-does-not-exist.png"),
            &|| true,
        );
        assert_eq!(
            result.expect_err("cancelled request").to_string(),
            "Carga cancelada."
        );
    }

    #[test]
    fn large_buffer_copy_cancels_between_chunks() {
        let source = vec![7; MAX_COPY_CHUNK_BYTES * 3];
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let result = clone_bytes_cancellable(&source, &|| {
            checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
        });
        assert_eq!(
            result.expect_err("cancelled chunked copy").to_string(),
            "Carga cancelada."
        );
    }

    #[test]
    fn rotated_frame_cancels_between_tiles() {
        let frame = Frame {
            width: 64,
            height: 64,
            pixels: Arc::new(vec![0; 64 * 64 * 4]),
            delay_ms: 100,
        };
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let result = transform_frame_cancellable(&frame, 6, &|| {
            checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
        });
        assert_eq!(
            result.expect_err("cancelled tiled transform").to_string(),
            "Carga cancelada."
        );
    }

    fn sample() -> Frame {
        Frame {
            width: 2,
            height: 1,
            pixels: Arc::new(vec![1, 2, 3, 4, 5, 6, 7, 8]),
            delay_ms: 100,
        }
    }

    #[test]
    fn rotate_preserves_pixels() {
        let rotated = rotate(&sample(), true);
        assert_eq!((rotated.width, rotated.height), (1, 2));
        assert_eq!(rotated.pixels, sample().pixels);
    }

    #[test]
    fn editing_history_restores_and_reapplies_pixels() {
        let original = sample();
        let mut document = ImageDocument {
            path: PathBuf::from("sample.png"),
            frames: vec![original.clone()],
            frame_index: 0,
            original_size: (2, 1),
            resolution_dpi: (96.0, 96.0),
            file_size: 8,
            format_name: "PNG".into(),
            metadata: Vec::new(),
            metadata_dirty: false,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        document.flip_horizontal();
        assert_eq!(
            document.current_frame().pixels,
            Arc::new(vec![5, 6, 7, 8, 1, 2, 3, 4])
        );
        assert!(document.dirty);
        assert!(document.undo());
        assert_eq!(document.current_frame().pixels, original.pixels);
        assert!(document.redo());
        assert_eq!(
            document.current_frame().pixels,
            Arc::new(vec![5, 6, 7, 8, 1, 2, 3, 4])
        );
        assert!(document.crop(0, 0, 1, 1));
        assert_eq!(
            (
                document.current_frame().width,
                document.current_frame().height
            ),
            (1, 1)
        );
        assert!(!document.crop(1, 0, 1, 1));
    }

    #[test]
    fn export_snapshot_shares_pixels_until_the_document_is_edited() {
        let mut document = ImageDocument {
            path: PathBuf::from("sample.png"),
            frames: vec![sample()],
            frame_index: 0,
            original_size: (2, 1),
            resolution_dpi: (96.0, 96.0),
            file_size: 8,
            format_name: "PNG".into(),
            metadata: Vec::new(),
            metadata_dirty: false,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        let snapshot = document.export_snapshot();
        assert!(Arc::ptr_eq(
            &document.current_frame().pixels,
            &snapshot.current_frame().pixels
        ));

        document.flip_horizontal();
        assert_eq!(
            snapshot.current_frame().pixels.as_slice(),
            &[1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_ne!(
            document.current_frame().pixels.as_slice(),
            snapshot.current_frame().pixels.as_slice()
        );
    }

    #[test]
    fn wic_round_trip_png() {
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let path = std::env::temp_dir().join(format!("kgen-view-test-{}.png", std::process::id()));
        let frame = Frame {
            width: 2,
            height: 1,
            pixels: Arc::new(vec![0, 0, 255, 255, 0, 255, 0, 255]),
            delay_ms: 100,
        };
        let mut document = ImageDocument {
            path: path.clone(),
            frames: vec![frame],
            frame_index: 0,
            original_size: (2, 1),
            resolution_dpi: (96.0, 96.0),
            file_size: 0,
            format_name: "PNG".into(),
            metadata: Vec::new(),
            metadata_dirty: false,
            dirty: true,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        document.save_as(&path).expect("encode PNG with WIC");
        let decoded = ImageDocument::decode(&path).expect("decode PNG with WIC");
        assert_eq!(
            (
                decoded.current_frame().width,
                decoded.current_frame().height
            ),
            (2, 1)
        );
        assert!(
            decoded
                .current_frame()
                .pixels
                .iter()
                .any(|value| *value != 0)
        );
        let converted_path = path.with_file_name(format!(
            "kgen-view-converted-test-{}.png",
            std::process::id()
        ));
        let png_format = ExportFormat {
            container: windows::Win32::Graphics::Imaging::GUID_ContainerFormatPng,
            name: "PNG".into(),
            extensions: vec!["png".into()],
        };
        document
            .export_snapshot()
            .export_copy_as(&converted_path, &png_format)
            .expect("convert PNG copy with WIC");
        let converted = ImageDocument::decode(&converted_path).expect("decode converted PNG");
        assert_eq!(
            converted.current_frame().pixels.as_slice(),
            document.current_frame().pixels.as_slice()
        );
        let _ = std::fs::remove_file(converted_path);
        let _ = std::fs::remove_file(path);
        if initialized {
            unsafe { CoUninitialize() }
        }
    }

    #[test]
    fn animated_gif_is_composited_and_timed() {
        let path = std::env::temp_dir().join(format!(
            "kgen-view-animation-test-{}.gif",
            std::process::id()
        ));
        {
            let file = std::fs::File::create(&path).expect("create GIF fixture");
            let mut encoder = gif::Encoder::new(file, 2, 1, &[]).expect("GIF encoder");
            let mut first_pixels = vec![255, 0, 0, 255, 0, 0, 0, 0];
            let mut first = gif::Frame::from_rgba_speed(2, 1, &mut first_pixels, 10);
            first.delay = 4;
            first.dispose = gif::DisposalMethod::Keep;
            encoder.write_frame(&first).expect("first GIF frame");
            let mut second_pixels = vec![0, 0, 0, 0, 0, 255, 0, 255];
            let mut second = gif::Frame::from_rgba_speed(2, 1, &mut second_pixels, 10);
            second.delay = 7;
            encoder.write_frame(&second).expect("second GIF frame");
        }
        let decoded = decode_gif(&path, &|| false).expect("decode animated GIF");
        assert_eq!(decoded.frame_count(), 2);
        assert_eq!(decoded.frames[0].delay_ms, 40);
        assert_eq!(decoded.frames[1].delay_ms, 70);
        assert_eq!(&decoded.frames[1].pixels[0..4], &[0, 0, 255, 255]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn benchmark_external_image_when_requested() {
        let Some(path) = std::env::var_os("KGEN_BENCH_IMAGE").map(std::path::PathBuf::from) else {
            return;
        };
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let started = std::time::Instant::now();
        let decoded = ImageDocument::decode(&path).expect("decode benchmark image");
        println!(
            "KGEN_DECODE_MS={:.3}; PIXELS={}x{}",
            started.elapsed().as_secs_f64() * 1000.0,
            decoded.current_frame().width,
            decoded.current_frame().height,
        );
        if initialized {
            unsafe { CoUninitialize() }
        }
    }
}
