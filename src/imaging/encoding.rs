use super::*;

impl ImageDocument {
    pub(super) fn write_container(
        &self,
        path: &Path,
        container: windows::core::GUID,
        extension: &str,
        metadata_dirty: bool,
    ) -> ImageResult<()> {
        let _span = crate::diagnostics::Span::new("export");
        static EXPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _writer = EXPORT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(source) = &self.source {
            source.verify()?;
        }
        let factory = Self::create_wic_factory()?;
        let temp_path = create_temp_sibling(path)?;
        let result = if self.source.is_some()
            && export::is_wic_source(&self.path)
            && container != windows::Win32::Graphics::Imaging::GUID_ContainerFormatGif
        {
            self.encode_native_wic(&factory, &temp_path, container, extension, metadata_dirty)
        } else {
            let materialized;
            let document = if self.is_preview || !self.edits.operations.is_empty() {
                materialized = self.render_view(
                    DecodeRequest::original(self.frame_index),
                    &|| false,
                    Some(&factory),
                )?;
                &materialized
            } else {
                self
            };
            document.encode_to_path(&factory, &temp_path, container, extension, metadata_dirty)
        };
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
pub(super) fn convert_encoder_source(
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

pub(super) fn valid_resolution((dpi_x, dpi_y): (f64, f64)) -> (f64, f64) {
    if dpi_x.is_finite() && dpi_y.is_finite() && dpi_x > 0.0 && dpi_y > 0.0 {
        (dpi_x, dpi_y)
    } else {
        (96.0, 96.0)
    }
}
