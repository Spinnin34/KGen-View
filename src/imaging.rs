use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Graphics::Imaging::{
            CLSID_WICImagingFactory2, GUID_ContainerFormatBmp, GUID_ContainerFormatJpeg,
            GUID_ContainerFormatPng, GUID_ContainerFormatTiff, GUID_WICPixelFormat32bppPBGRA,
            IWICBitmapFrameDecode, IWICBitmapSource, IWICColorContext, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnDemand,
        },
        System::{
            Com::StructuredStorage::{PROPVARIANT, PropVariantClear},
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Variant::{VT_BSTR, VT_I2, VT_LPSTR, VT_LPWSTR, VT_UI2, VT_UI4},
        },
    },
    core::{Interface, PCWSTR, w},
};

const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FRAMES: u32 = 512;

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
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
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
    pub file_size: u64,
    pub format_name: String,
    pub metadata: Vec<(String, String)>,
    pub dirty: bool,
    undo: Vec<DocumentSnapshot>,
    redo: Vec<DocumentSnapshot>,
}

impl ImageDocument {
    pub fn decode(path: &Path) -> ImageResult<Self> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("gif"))
        {
            return decode_gif(&path);
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("svg"))
        {
            return decode_svg(&path);
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jxl"))
        {
            return decode_jxl(&path);
        }
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER)? };
        let wide = wide_path(&path);
        let decoder = unsafe {
            factory.CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?
        };
        let frame_count = unsafe { decoder.GetFrameCount()? }.min(MAX_FRAMES);
        if frame_count == 0 {
            return Err(ImageError("El archivo no contiene imágenes.".into()));
        }

        let mut frames = Vec::with_capacity(frame_count as usize);
        let mut metadata = Vec::new();
        let mut total_bytes = 0u64;
        for index in 0..frame_count {
            let source_frame = unsafe { decoder.GetFrame(index)? };
            if index == 0 {
                for (label, query) in [
                    ("Fabricante", w!("/app1/ifd/{ushort=271}")),
                    ("Cámara", w!("/app1/ifd/{ushort=272}")),
                    ("Software", w!("/app1/ifd/{ushort=305}")),
                    ("Fecha original", w!("/app1/ifd/exif/{ushort=36867}")),
                    ("Autor", w!("/app1/ifd/{ushort=315}")),
                    ("Descripción", w!("/app1/ifd/{ushort=270}")),
                ] {
                    if let Some(value) = read_metadata_string(&source_frame, query) {
                        metadata.push((label.into(), value));
                    }
                }
            }
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
            if total_bytes > MAX_DECODE_BYTES {
                return Err(ImageError(format!(
                    "La imagen necesita más de {} MiB para decodificarse.",
                    MAX_DECODE_BYTES / 1024 / 1024
                )));
            }

            let source = color_managed_source(&factory, &source_frame)?;
            let mut pixels = vec![0u8; len as usize];
            unsafe { source.CopyPixels(std::ptr::null(), stride, &mut pixels)? };
            let orientation = read_metadata_u32(&source_frame, w!("/app1/ifd/{ushort=274}"))
                .or_else(|| read_metadata_u32(&source_frame, w!("/ifd/{ushort=274}")))
                .unwrap_or(1);
            frames.push(apply_orientation(
                Frame {
                    width,
                    height,
                    pixels,
                    delay_ms: 100,
                },
                orientation,
            ));
        }

        let file_size = fs::metadata(&path).map(|value| value.len()).unwrap_or(0);
        let format_name = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("?")
            .to_ascii_uppercase();
        Ok(Self {
            path,
            frames,
            frame_index: 0,
            file_size,
            format_name,
            metadata,
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
        let bytes: usize = self.frames.iter().map(Frame::byte_len).sum();
        if bytes <= 128 * 1024 * 1024 {
            self.undo.push(DocumentSnapshot {
                frames: self.frames.clone(),
            });
            while self.undo.len() > 8 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(DocumentSnapshot {
            frames: std::mem::replace(&mut self.frames, snapshot.frames),
        });
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
            let row = (frame.width * 4) as usize;
            for y in 0..frame.height as usize {
                for x in 0..frame.width as usize / 2 {
                    for channel in 0..4 {
                        frame.pixels.swap(
                            y * row + x * 4 + channel,
                            y * row + (frame.width as usize - 1 - x) * 4 + channel,
                        );
                    }
                }
            }
        }
    }

    pub fn flip_vertical(&mut self) {
        self.checkpoint();
        for frame in &mut self.frames {
            let row = (frame.width * 4) as usize;
            for y in 0..frame.height as usize / 2 {
                let other = frame.height as usize - 1 - y;
                for x in 0..row {
                    frame.pixels.swap(y * row + x, other * row + x);
                }
            }
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
            frame.pixels = pixels;
        }
        true
    }

    pub fn save_as(&mut self, path: &Path) -> ImageResult<()> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let container = match extension.as_str() {
            "jpg" | "jpeg" | "jpe" => GUID_ContainerFormatJpeg,
            "png" => GUID_ContainerFormatPng,
            "bmp" | "dib" => GUID_ContainerFormatBmp,
            "tif" | "tiff" => GUID_ContainerFormatTiff,
            _ => {
                return Err(ImageError(
                    "Formato de salida no compatible. Usa PNG, JPEG, BMP o TIFF.".into(),
                ));
            }
        };
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER)? };
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
            encoded.SetResolution(96.0, 96.0)?;
            let bitmap = factory.CreateBitmapFromMemory(
                frame.width,
                frame.height,
                &GUID_WICPixelFormat32bppPBGRA,
                frame.width * 4,
                &frame.pixels,
            )?;
            encoded.WriteSource(&bitmap, std::ptr::null())?;
            encoded.Commit()?;
            encoder.Commit()?;
        }
        self.path = path.to_path_buf();
        self.file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
        self.format_name = extension.to_ascii_uppercase();
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        Ok(())
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

fn color_managed_source(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> ImageResult<IWICBitmapSource> {
    let mut count = 0u32;
    let mut contexts: Vec<Option<IWICColorContext>> = Vec::new();
    let has_profile =
        unsafe { frame.GetColorContexts(&mut contexts, &mut count) }.is_ok() && count > 0;
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

fn read_metadata_u32(frame: &IWICBitmapFrameDecode, query: PCWSTR) -> Option<u32> {
    let reader = unsafe { frame.GetMetadataQueryReader() }.ok()?;
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

fn read_metadata_string(frame: &IWICBitmapFrameDecode, query: PCWSTR) -> Option<String> {
    let reader = unsafe { frame.GetMetadataQueryReader() }.ok()?;
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

fn apply_orientation(frame: Frame, orientation: u32) -> Frame {
    match orientation {
        2 => flip_frame_horizontal(&frame),
        3 => rotate(&rotate(&frame, true), true),
        4 => flip_frame_vertical(&frame),
        5 => rotate(&flip_frame_horizontal(&frame), false),
        6 => rotate(&frame, true),
        7 => rotate(&flip_frame_horizontal(&frame), true),
        8 => rotate(&frame, false),
        _ => frame,
    }
}

fn flip_frame_horizontal(frame: &Frame) -> Frame {
    let mut result = frame.clone();
    let row = (result.width * 4) as usize;
    for y in 0..result.height as usize {
        for x in 0..result.width as usize / 2 {
            for channel in 0..4 {
                result.pixels.swap(
                    y * row + x * 4 + channel,
                    y * row + (result.width as usize - 1 - x) * 4 + channel,
                );
            }
        }
    }
    result
}

fn flip_frame_vertical(frame: &Frame) -> Frame {
    let mut result = frame.clone();
    let row = (result.width * 4) as usize;
    for y in 0..result.height as usize / 2 {
        let other = result.height as usize - 1 - y;
        for x in 0..row {
            result.pixels.swap(y * row + x, other * row + x);
        }
    }
    result
}

fn decode_gif(path: &Path) -> ImageResult<ImageDocument> {
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
        let Some(frame) = decoder
            .read_next_frame()
            .map_err(|error| ImageError(error.to_string()))?
        else {
            break;
        };
        match previous_disposal {
            DisposalMethod::Background => clear_rect(&mut canvas, width, previous_rect),
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
        restore_canvas = if frame.dispose == DisposalMethod::Previous {
            Some(canvas.clone())
        } else {
            None
        };
        for y in 0..frame_height {
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
            pixels: canvas.clone(),
            delay_ms,
        });
        if (frames.len() as u64).saturating_mul(canvas_len) > MAX_DECODE_BYTES {
            return Err(ImageError(
                "La animación GIF supera el límite de 512 MiB.".into(),
            ));
        }
        previous_disposal = frame.dispose;
        previous_rect = (left, top, frame_width, frame_height);
    }
    if frames.is_empty() {
        return Err(ImageError("El GIF no contiene fotogramas.".into()));
    }
    let file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
    Ok(ImageDocument {
        path: path.to_path_buf(),
        frames,
        frame_index: 0,
        file_size,
        format_name: "GIF".into(),
        metadata: Vec::new(),
        dirty: false,
        undo: Vec::new(),
        redo: Vec::new(),
    })
}

fn decode_svg(path: &Path) -> ImageResult<ImageDocument> {
    let data = fs::read(path)?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt)
        .map_err(|error| ImageError(format!("Error decoding SVG: {error}")))?;
    let size = tree.size();
    let width = (size.width().ceil() as u32).max(1);
    let height = (size.height().ceil() as u32).max(1);
    validate_dimensions(width, height)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| ImageError("No se pudo crear mapa de píxeles para SVG.".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    let raw_rgba = pixmap.data();
    let pixel_count = (width as usize) * (height as usize);
    let mut pixels = vec![0u8; pixel_count * 4];
    for i in 0..pixel_count {
        let r = raw_rgba[i * 4];
        let g = raw_rgba[i * 4 + 1];
        let b = raw_rgba[i * 4 + 2];
        let a = raw_rgba[i * 4 + 3];
        pixels[i * 4] = b;
        pixels[i * 4 + 1] = g;
        pixels[i * 4 + 2] = r;
        pixels[i * 4 + 3] = a;
    }

    let file_size = fs::metadata(path).map(|value| value.len()).unwrap_or(0);
    Ok(ImageDocument {
        path: path.to_path_buf(),
        frames: vec![Frame {
            width,
            height,
            pixels,
            delay_ms: 100,
        }],
        frame_index: 0,
        file_size,
        format_name: "SVG".into(),
        metadata: Vec::new(),
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
    let mut samples = vec![0u8; sample_count];
    let written = stream.write_to_buffer(&mut samples);
    if written != sample_count {
        return Err(ImageError(
            "No se pudieron leer los píxeles JPEG XL.".into(),
        ));
    }

    let mut pixels = vec![0u8; pixel_bytes as usize];
    for (source, target) in samples
        .chunks_exact(channels)
        .zip(pixels.chunks_exact_mut(4))
    {
        let (red, green, blue, alpha) = if channels == 4 {
            (source[0], source[1], source[2], source[3])
        } else {
            (source[0], source[1], source[2], 255)
        };
        let premultiply = |color: u8| ((u16::from(color) * u16::from(alpha) + 127) / 255) as u8;
        target.copy_from_slice(&[
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha,
        ]);
    }

    Ok(ImageDocument {
        path: path.to_path_buf(),
        frames: vec![Frame {
            width,
            height,
            pixels,
            delay_ms: 100,
        }],
        frame_index: 0,
        file_size,
        format_name: "JPEG XL".into(),
        metadata: Vec::new(),
        dirty: false,
        undo: Vec::new(),
        redo: Vec::new(),
    })
}

fn clear_rect(canvas: &mut [u8], canvas_width: u32, rect: (u32, u32, u32, u32)) {
    let (left, top, width, height) = rect;
    for y in top..top.saturating_add(height) {
        let start = ((y * canvas_width + left) * 4) as usize;
        let end = start.saturating_add((width * 4) as usize).min(canvas.len());
        canvas[start..end].fill(0);
    }
}

fn rotate(frame: &Frame, clockwise: bool) -> Frame {
    let new_width = frame.height;
    let new_height = frame.width;
    let mut pixels = vec![0; (new_width * new_height * 4) as usize];
    for y in 0..frame.height {
        for x in 0..frame.width {
            let (nx, ny) = if clockwise {
                (frame.height - 1 - y, x)
            } else {
                (y, frame.width - 1 - x)
            };
            let src = ((y * frame.width + x) * 4) as usize;
            let dst = ((ny * new_width + nx) * 4) as usize;
            pixels[dst..dst + 4].copy_from_slice(&frame.pixels[src..src + 4]);
        }
    }
    Frame {
        width: new_width,
        height: new_height,
        pixels,
        delay_ms: frame.delay_ms,
    }
}

pub fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    fn sample() -> Frame {
        Frame {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
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
            file_size: 8,
            format_name: "PNG".into(),
            metadata: Vec::new(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        document.flip_horizontal();
        assert_eq!(
            document.current_frame().pixels,
            vec![5, 6, 7, 8, 1, 2, 3, 4]
        );
        assert!(document.dirty);
        assert!(document.undo());
        assert_eq!(document.current_frame().pixels, original.pixels);
        assert!(document.redo());
        assert_eq!(
            document.current_frame().pixels,
            vec![5, 6, 7, 8, 1, 2, 3, 4]
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
    fn wic_round_trip_png() {
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let path = std::env::temp_dir().join(format!("kgen-view-test-{}.png", std::process::id()));
        let frame = Frame {
            width: 2,
            height: 1,
            pixels: vec![0, 0, 255, 255, 0, 255, 0, 255],
            delay_ms: 100,
        };
        let mut document = ImageDocument {
            path: path.clone(),
            frames: vec![frame],
            frame_index: 0,
            file_size: 0,
            format_name: "PNG".into(),
            metadata: Vec::new(),
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
        let decoded = decode_gif(&path).expect("decode animated GIF");
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
