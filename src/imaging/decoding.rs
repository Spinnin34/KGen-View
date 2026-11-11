use super::*;

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
        let mut request = DecodeRequest::preview((1920, 1080));
        request.max_bytes = MAX_PREFETCH_DECODE_BYTES as usize;
        Some(Self::decode_request(path, should_cancel, factory, &request))
    }

    #[cfg(test)]
    pub fn decode(path: &Path) -> ImageResult<Self> {
        Self::decode_cancellable(path, &|| false)
    }

    #[cfg(test)]
    pub fn decode_cancellable(path: &Path, should_cancel: &impl Fn() -> bool) -> ImageResult<Self> {
        Self::decode_cancellable_with_limit(path, should_cancel, MAX_DECODE_BYTES, None)
    }

    #[cfg(test)]
    fn decode_cancellable_with_limit(
        path: &Path,
        should_cancel: &impl Fn() -> bool,
        max_decode_bytes: u64,
        wic_factory: Option<&IWICImagingFactory>,
    ) -> ImageResult<Self> {
        let mut request = DecodeRequest::original(0);
        request.max_bytes = max_decode_bytes as usize;
        Self::decode_request(path, should_cancel, wic_factory, &request)
    }

    pub fn decode_request(
        path: &Path,
        should_cancel: &impl Fn() -> bool,
        wic_factory: Option<&IWICImagingFactory>,
        request: &DecodeRequest,
    ) -> ImageResult<Self> {
        let _span = crate::diagnostics::Span::new("decode");
        let max_decode_bytes = request.max_bytes as u64;
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let image_source = ImageSource::open(&path)?;
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("gif"))
        {
            return decode_gif(&path, should_cancel, request);
        }
        gif_source::clear();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("svg"))
        {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            return decode_svg(&path, should_cancel, request);
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jxl"))
        {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            return decode_jxl(&path, should_cancel, request);
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

        let mut frames = Vec::with_capacity(1);
        let mut original_size = (0, 0);
        let mut rendered_region = (0, 0, 0, 0);
        let mut metadata = Vec::new();
        let mut resolution_dpi = (96.0, 96.0);
        for index in [request.page.min(frame_count as usize - 1) as u32] {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let source_frame = unsafe { decoder.GetFrame(index)? };
            let mut width = 0;
            let mut height = 0;
            unsafe { source_frame.GetSize(&mut width, &mut height)? };
            validate_dimensions(width, height)?;
            // Reuse the frame's metadata reader for its fields instead of
            // creating a new COM reader for every individual property.
            let metadata_reader = if has_exif_metadata {
                unsafe { source_frame.GetMetadataQueryReader().ok() }
            } else {
                None
            };
            {
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
            if let Some(reader) = metadata_reader.as_ref() {
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
            let orientation = metadata_reader
                .as_ref()
                .and_then(|reader| read_metadata_u32(reader, w!("/app1/ifd/{ushort=274}")))
                .or_else(|| {
                    metadata_reader
                        .as_ref()
                        .and_then(|reader| read_metadata_u32(reader, w!("/ifd/{ushort=274}")))
                })
                .unwrap_or(1);
            original_size = if matches!(orientation, 5..=8) {
                (height, width)
            } else {
                (width, height)
            };
            if matches!(orientation, 5..=8) {
                resolution_dpi = (resolution_dpi.1, resolution_dpi.0);
            }
            let layout = request.layout(original_size);
            rendered_region = layout.region;
            let decoded_source = wic_view::native_thumbnail_source(
                &factory,
                &source_frame,
                original_size,
                orientation,
                request,
                should_cancel,
            )?
            .unwrap_or(source_frame.cast()?);
            let source = color_managed_source_from(&factory, &source_frame, decoded_source)?;
            let source = prepare_wic_view(&factory, source, orientation, original_size, request)?;
            let (width, height) = layout.size;
            let stride = width
                .checked_mul(4)
                .ok_or_else(|| ImageError("Dimensiones no válidas.".into()))?;
            let len = u64::from(stride) * u64::from(height);
            if len > max_decode_bytes {
                return Err(ImageError("Imagen demasiado grande.".into()));
            }
            let mut pixels = PixelBuffer::zeroed(len as usize)?;
            copy_pixels_cancellable(
                &source,
                width,
                height,
                stride,
                pixels.try_make_mut()?,
                should_cancel,
            )?;
            frames.push(Frame {
                width,
                height,
                pixels,
                delay_ms: 100,
            });
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
            frame_index: request.page.min(frame_count as usize - 1),
            original_size,
            resolution_dpi,
            file_size,
            format_name,
            metadata,
            metadata_dirty: false,
            dirty: false,
            id: DocumentId::new(),
            revision: 0,
            is_preview: request.layout(original_size).size != original_size
                || !request.operations.is_empty()
                || request.region.is_some(),
            rendered_region,
            source_frame_count: frame_count as usize,
            source_lease: None,
            source: Some(image_source),
            edits: EditHistory::from_operations(request.operations.clone()),
        })
    }
}
pub(super) fn color_managed_source(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> ImageResult<IWICBitmapSource> {
    color_managed_source_from(factory, frame, frame.cast()?)
}

pub(super) fn color_managed_source_from(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
    source: IWICBitmapSource,
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
                        &source,
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
            &source,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
    }
    Ok(converter.cast()?)
}

pub(super) fn copy_pixels_cancellable(
    source: &IWICBitmapSource,
    width: u32,
    height: u32,
    stride: u32,
    pixels: &mut [u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let _span = crate::diagnostics::Span::new("copy_pixels");
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
