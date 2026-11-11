use super::*;

pub(super) fn decode_jxl(
    path: &Path,
    should_cancel: &impl Fn() -> bool,
    request: &DecodeRequest,
) -> ImageResult<ImageDocument> {
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let tracker_bytes = (256 * crate::resources::MIB)
        .min(crate::resources::CPU_LIMIT.saturating_sub(crate::resources::cpu_budget().used()));
    let _decoder_memory = crate::resources::cpu_budget().reserve(tracker_bytes)?;
    let file_size = fs::metadata(path)?.len();
    if file_size > MAX_DECODE_BYTES {
        return Err(ImageError(
            "El archivo JPEG XL supera el límite de 512 MiB.".into(),
        ));
    }

    let tracker = jxl_oxide::AllocTracker::with_limit(tracker_bytes);
    let mut decoder = jxl_oxide::JxlImage::builder()
        .alloc_tracker(tracker)
        .pool(jxl_pool())
        .open(path)
        .map_err(|error| ImageError(format!("No se pudo decodificar JPEG XL: {error}")))?;
    let width = decoder.width();
    let height = decoder.height();
    validate_dimensions(width, height)?;

    let original = (width, height);
    let (left, top, right, bottom) = source_region(original, request);
    decoder.set_image_region(jxl_oxide::CropInfo {
        left,
        top,
        width: right - left,
        height: bottom - top,
    });
    let (width, height) = (right - left, bottom - top);
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
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let mut pixels = PixelBuffer::zeroed(pixel_bytes as usize)?;
    let written = stream.write_to_buffer(&mut pixels.try_make_mut()?[..sample_count]);
    if written != sample_count {
        return Err(ImageError(
            "No se pudieron leer los píxeles JPEG XL.".into(),
        ));
    }

    if channels == 4 {
        for chunk in pixels.try_make_mut()?.chunks_mut(MAX_COPY_CHUNK_BYTES) {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            for pixel in chunk.as_chunks_mut::<4>().0 {
                let [red, green, blue, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
                let premultiply =
                    |color: u8| ((u16::from(color) * u16::from(alpha) + 127) / 255) as u8;
                pixel.copy_from_slice(&[
                    premultiply(blue),
                    premultiply(green),
                    premultiply(red),
                    alpha,
                ]);
            }
        }
    } else {
        let pixel_count = (width as usize) * (height as usize);
        let pixels = pixels.try_make_mut()?;
        for index in (0..pixel_count).rev() {
            if index % 65536 == 0 && should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let source = index * 3;
            let target = index * 4;
            let red = pixels[source];
            let green = pixels[source + 1];
            let blue = pixels[source + 2];
            pixels[target..target + 4].copy_from_slice(&[blue, green, red, 255]);
        }
    }

    drop(stream);
    drop(render);
    drop(decoder);
    drop(_decoder_memory);
    let frame = Frame {
        width,
        height,
        pixels,
        delay_ms: 100,
    };
    let frame = render_raster_region(&frame, original, (left, top), request, should_cancel)?;
    Ok(raster_document(
        path, frame, original, 1, 0, "JPEG XL", request,
    ))
}

fn jxl_pool() -> jxl_oxide::JxlThreadPool {
    static POOL: std::sync::OnceLock<jxl_oxide::JxlThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| jxl_oxide::JxlThreadPool::rayon(Some(2)))
        .clone()
}
