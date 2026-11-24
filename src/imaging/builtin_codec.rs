use super::*;
use image::GenericImageView;

const MAX_BUILTIN_DECODE_BYTES: usize = crate::resources::CPU_LIMIT;

pub(super) fn format_for_path(path: &Path) -> Option<image::ImageFormat> {
    let extension = path.extension()?.to_str()?;
    let format = image::ImageFormat::from_extension(extension)?;
    (format.can_read() && format.reading_enabled()).then_some(format)
}

pub(super) fn should_decode_before_wic(format: image::ImageFormat) -> bool {
    matches!(
        format,
        image::ImageFormat::Tga
            | image::ImageFormat::OpenExr
            | image::ImageFormat::Hdr
            | image::ImageFormat::Pnm
            | image::ImageFormat::Farbfeld
            | image::ImageFormat::Qoi
    )
}

pub(super) fn export_formats() -> impl Iterator<Item = image::ImageFormat> {
    image::ImageFormat::all().filter(|format| {
        format.can_read()
            && format.can_write()
            && format.reading_enabled()
            && format.writing_enabled()
    })
}

pub(super) fn display_name(format: image::ImageFormat) -> &'static str {
    match format {
        image::ImageFormat::Png => "PNG (KGen)",
        image::ImageFormat::Jpeg => "JPEG (KGen)",
        image::ImageFormat::Gif => "GIF (KGen)",
        image::ImageFormat::WebP => "WebP (KGen)",
        image::ImageFormat::Tiff => "TIFF (KGen)",
        image::ImageFormat::Tga => "TGA",
        image::ImageFormat::Bmp => "BMP (KGen)",
        image::ImageFormat::Ico => "Icono (KGen)",
        image::ImageFormat::Hdr => "Radiance HDR",
        image::ImageFormat::OpenExr => "OpenEXR",
        image::ImageFormat::Pnm => "Portable Anymap",
        image::ImageFormat::Farbfeld => "Farbfeld",
        image::ImageFormat::Avif => "AVIF",
        image::ImageFormat::Qoi => "QOI",
        image::ImageFormat::Dds => "DDS",
        #[allow(deprecated)]
        image::ImageFormat::Pcx => "PCX",
        _ => "Imagen",
    }
}

pub(super) fn decode(
    path: &Path,
    format: image::ImageFormat,
    should_cancel: &impl Fn() -> bool,
    request: &DecodeRequest,
) -> ImageResult<ImageDocument> {
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }

    let image_source = ImageSource::open(path)?;
    let dimensions = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| ImageError(error.to_string()))?
        .into_dimensions()
        .map_err(|error| ImageError(error.to_string()))?;
    validate_dimensions(dimensions.0, dimensions.1)?;

    let pixels = u64::from(dimensions.0)
        .checked_mul(u64::from(dimensions.1))
        .ok_or_else(|| ImageError("Dimensiones no válidas.".into()))?;
    let bytes_per_pixel = match format {
        image::ImageFormat::OpenExr | image::ImageFormat::Tiff => 16,
        image::ImageFormat::Hdr => 12,
        image::ImageFormat::Farbfeld | image::ImageFormat::Pnm | image::ImageFormat::Png => 8,
        _ => 4,
    };
    let decoder_bytes = pixels
        .checked_mul(bytes_per_pixel)
        .filter(|bytes| *bytes <= MAX_BUILTIN_DECODE_BYTES as u64)
        .ok_or_else(|| {
            ImageError("La imagen supera el límite de memoria de los códecs integrados.".into())
        })? as usize;
    if request.prefetch_only && decoder_bytes > crate::resources::PREFETCH_LIMIT {
        return Err(ImageError(
            "Imagen demasiado grande para la precarga.".into(),
        ));
    }

    let decoder_memory = crate::resources::cpu_budget()
        .reserve(decoder_bytes)
        .map_err(ImageError::from)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(100_000);
    limits.max_image_height = Some(100_000);
    limits.max_alloc = Some(decoder_bytes as u64);
    let reader = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| ImageError(error.to_string()))?;
    let mut reader = reader;
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| ImageError(error.to_string()))?;
    if should_cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }

    let (width, height) = decoded.dimensions();
    validate_dimensions(width, height)?;
    let output_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ImageError("Dimensiones no válidas.".into()))?;
    let mut frame_memory = crate::resources::cpu_budget()
        .reserve(output_bytes)
        .map_err(ImageError::from)?;
    let mut rgba = match format {
        image::ImageFormat::Hdr | image::ImageFormat::OpenExr => into_tone_mapped_rgba8(decoded)?,
        _ => decoded.into_rgba8(),
    }
    .into_raw();
    if rgba.len() != output_bytes {
        return Err(ImageError(
            "El códec devolvió un búfer de píxeles no válido.".into(),
        ));
    }
    if rgba.capacity() > output_bytes {
        frame_memory = crate::resources::cpu_budget()
            .reserve(rgba.capacity())
            .map_err(ImageError::from)?;
    }
    drop(decoder_memory);
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let [red, green, blue, alpha] = *pixel;
        let premultiply = |channel: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
        pixel.copy_from_slice(&[
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha,
        ]);
    }
    let pixels = PixelBuffer::with_reservation(rgba, frame_memory).map_err(ImageError::from)?;
    let source_frame = Frame {
        width,
        height,
        pixels,
        delay_ms: 100,
    };
    let frame = render_raster_region(&source_frame, dimensions, (0, 0), request, should_cancel)?;
    let mut document = raster_document(
        path,
        frame,
        dimensions,
        1,
        request.page,
        &path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("img")
            .to_ascii_uppercase(),
        request,
    );
    image_source.verify()?;
    document.source = Some(image_source);
    Ok(document)
}

fn into_tone_mapped_rgba8(image: image::DynamicImage) -> ImageResult<image::RgbaImage> {
    let (width, height) = image.dimensions();
    let capacity = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ImageError("Dimensiones no válidas.".into()))?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(capacity)
        .map_err(|_| ImageError("No se pudo reservar memoria para la imagen.".into()))?;
    match image {
        image::DynamicImage::ImageRgb32F(pixels) => {
            for pixel in pixels.pixels() {
                let [red, green, blue] = pixel.0;
                rgba.extend_from_slice(&[
                    linear_to_srgb(red),
                    linear_to_srgb(green),
                    linear_to_srgb(blue),
                    255,
                ]);
            }
        }
        image::DynamicImage::ImageRgba32F(pixels) => {
            for pixel in pixels.pixels() {
                let [red, green, blue, alpha] = pixel.0;
                rgba.extend_from_slice(&[
                    linear_to_srgb(red),
                    linear_to_srgb(green),
                    linear_to_srgb(blue),
                    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]);
            }
        }
        other => {
            for pixel in other.into_rgba32f().pixels() {
                let [red, green, blue, alpha] = pixel.0;
                rgba.extend_from_slice(&[
                    linear_to_srgb(red),
                    linear_to_srgb(green),
                    linear_to_srgb(blue),
                    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]);
            }
        }
    }
    image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| ImageError("No se pudo convertir el rango HDR de la imagen.".into()))
}

fn linear_to_srgb(value: f32) -> u8 {
    let linear = if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    let mapped = linear / (1.0 + linear);
    let encoded = if mapped <= 0.003_130_8 {
        mapped * 12.92
    } else {
        1.055 * mapped.powf(1.0 / 2.4) - 0.055
    };
    (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(super) fn write(
    document: &ImageDocument,
    path: &Path,
    format: image::ImageFormat,
    extension: &str,
) -> ImageResult<()> {
    if format == image::ImageFormat::Gif {
        return document.encode_gif_to_path(path);
    }
    if format == image::ImageFormat::Pnm {
        return write_pnm(document, path, extension);
    }
    if format == image::ImageFormat::Hdr {
        return write_hdr(document, path);
    }
    if format == image::ImageFormat::Farbfeld {
        return write_farbfeld(document, path);
    }
    if format == image::ImageFormat::OpenExr {
        return write_openexr(document, path);
    }
    let frame = document.current_frame();
    let channels = if format == image::ImageFormat::Jpeg {
        3
    } else {
        4
    };
    let length = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| ImageError("Dimensiones no válidas para la exportación.".into()))?;
    let memory = crate::resources::cpu_budget()
        .reserve(length)
        .map_err(ImageError::from)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ImageError("No se pudo reservar memoria para exportar.".into()))?;
    for bgra in frame.pixels.as_chunks::<4>().0 {
        let [blue, green, red, alpha] = *bgra;
        let flatten = |channel: u8| (u16::from(channel) + 255 - u16::from(alpha)).min(255) as u8;
        if channels == 3 {
            pixels.extend_from_slice(&[flatten(red), flatten(green), flatten(blue)]);
        } else {
            let unpremultiply = |channel: u8| {
                if alpha == 0 {
                    0
                } else {
                    ((u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255)
                        as u8
                }
            };
            pixels.extend_from_slice(&[
                unpremultiply(red),
                unpremultiply(green),
                unpremultiply(blue),
                alpha,
            ]);
        }
    }
    let image = if channels == 3 {
        let image = image::RgbImage::from_raw(frame.width, frame.height, pixels)
            .ok_or_else(|| ImageError("No se pudo preparar la imagen para JPEG.".into()))?;
        image::DynamicImage::ImageRgb8(image)
    } else {
        let image = image::RgbaImage::from_raw(frame.width, frame.height, pixels)
            .ok_or_else(|| ImageError("No se pudo preparar la imagen para exportar.".into()))?;
        image::DynamicImage::ImageRgba8(image)
    };
    let mut output = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    image
        .write_to(&mut output, format)
        .map_err(|error| ImageError(error.to_string()))?;
    output.sync_all()?;
    drop(memory);
    Ok(())
}

fn write_pnm(document: &ImageDocument, path: &Path, extension: &str) -> ImageResult<()> {
    use image::codecs::pnm::{PnmEncoder, PnmSubtype, SampleEncoding};

    let frame = document.current_frame();
    let (color, subtype, channels) = match extension {
        "pbm" => (
            image::ExtendedColorType::L8,
            PnmSubtype::Bitmap(SampleEncoding::Binary),
            1,
        ),
        "pgm" => (
            image::ExtendedColorType::L8,
            PnmSubtype::Graymap(SampleEncoding::Binary),
            1,
        ),
        "pam" => (image::ExtendedColorType::Rgba8, PnmSubtype::ArbitraryMap, 4),
        _ => (
            image::ExtendedColorType::Rgb8,
            PnmSubtype::Pixmap(SampleEncoding::Binary),
            3,
        ),
    };
    let count = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| ImageError("Dimensiones no válidas para PNM.".into()))?;
    let length = count
        .checked_mul(channels)
        .ok_or_else(|| ImageError("Dimensiones no válidas para PNM.".into()))?;
    let _memory = crate::resources::cpu_budget()
        .reserve(length)
        .map_err(ImageError::from)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ImageError("No se pudo reservar memoria para PNM.".into()))?;
    for bgra in frame.pixels.as_chunks::<4>().0 {
        let [blue, green, red, alpha] = *bgra;
        let flatten = |channel: u8| (u16::from(channel) + 255 - u16::from(alpha)).min(255) as u8;
        let red = flatten(red);
        let green = flatten(green);
        let blue = flatten(blue);
        match extension {
            "pbm" => {
                let luma = (54u16 * u16::from(red)
                    + 183u16 * u16::from(green)
                    + 19u16 * u16::from(blue)
                    + 128)
                    >> 8;
                pixels.push(u8::from(luma < 128));
            }
            "pgm" => pixels.push(
                ((54u16 * u16::from(red)
                    + 183u16 * u16::from(green)
                    + 19u16 * u16::from(blue)
                    + 128)
                    >> 8) as u8,
            ),
            "pam" => pixels.extend_from_slice(&[
                unpremultiply(red, alpha),
                unpremultiply(green, alpha),
                unpremultiply(blue, alpha),
                alpha,
            ]),
            _ => pixels.extend_from_slice(&[red, green, blue]),
        }
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    PnmEncoder::new(&mut output)
        .with_subtype(subtype)
        .encode(&pixels[..], frame.width, frame.height, color)
        .map_err(|error| ImageError(error.to_string()))?;
    output.sync_all()?;
    Ok(())
}

fn write_hdr(document: &ImageDocument, path: &Path) -> ImageResult<()> {
    let frame = document.current_frame();
    let count = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| ImageError("Dimensiones no válidas para HDR.".into()))?;
    let bytes = count
        .checked_mul(std::mem::size_of::<image::Rgb<f32>>())
        .ok_or_else(|| ImageError("Dimensiones no válidas para HDR.".into()))?;
    let _memory = crate::resources::cpu_budget()
        .reserve(bytes)
        .map_err(ImageError::from)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| ImageError("No se pudo reservar memoria para HDR.".into()))?;
    for bgra in frame.pixels.as_chunks::<4>().0 {
        let [blue, green, red, alpha] = *bgra;
        pixels.push(image::Rgb([
            srgb_to_linear(flatten(red, alpha)),
            srgb_to_linear(flatten(green, alpha)),
            srgb_to_linear(flatten(blue, alpha)),
        ]));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    image::codecs::hdr::HdrEncoder::new(&mut output)
        .encode(&pixels, frame.width as usize, frame.height as usize)
        .map_err(|error| ImageError(error.to_string()))?;
    output.sync_all()?;
    Ok(())
}

fn write_farbfeld(document: &ImageDocument, path: &Path) -> ImageResult<()> {
    let frame = document.current_frame();
    let count = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| ImageError("Dimensiones no válidas para Farbfeld.".into()))?;
    let length = count
        .checked_mul(4)
        .ok_or_else(|| ImageError("Dimensiones no válidas para Farbfeld.".into()))?;
    let bytes = length
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| ImageError("Dimensiones no válidas para Farbfeld.".into()))?;
    let _memory = crate::resources::cpu_budget()
        .reserve(bytes)
        .map_err(ImageError::from)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ImageError("No se pudo reservar memoria para Farbfeld.".into()))?;
    for bgra in frame.pixels.as_chunks::<4>().0 {
        let [blue, green, red, alpha] = *bgra;
        pixels.extend_from_slice(&[
            u16::from(unpremultiply(red, alpha)) * 257,
            u16::from(unpremultiply(green, alpha)) * 257,
            u16::from(unpremultiply(blue, alpha)) * 257,
            u16::from(alpha) * 257,
        ]);
    }
    let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
        frame.width,
        frame.height,
        pixels,
    )
    .ok_or_else(|| ImageError("No se pudo preparar la imagen Farbfeld.".into()))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    image::DynamicImage::ImageRgba16(image)
        .write_to(&mut output, image::ImageFormat::Farbfeld)
        .map_err(|error| ImageError(error.to_string()))?;
    output.sync_all()?;
    Ok(())
}

fn write_openexr(document: &ImageDocument, path: &Path) -> ImageResult<()> {
    let frame = document.current_frame();
    let count = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| ImageError("Dimensiones no válidas para OpenEXR.".into()))?;
    let length = count
        .checked_mul(4)
        .ok_or_else(|| ImageError("Dimensiones no válidas para OpenEXR.".into()))?;
    let bytes = length
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| ImageError("Dimensiones no válidas para OpenEXR.".into()))?;
    let _memory = crate::resources::cpu_budget()
        .reserve(bytes)
        .map_err(ImageError::from)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ImageError("No se pudo reservar memoria para OpenEXR.".into()))?;
    for bgra in frame.pixels.as_chunks::<4>().0 {
        let [blue, green, red, alpha] = *bgra;
        pixels.extend_from_slice(&[
            srgb_to_linear(unpremultiply(red, alpha)),
            srgb_to_linear(unpremultiply(green, alpha)),
            srgb_to_linear(unpremultiply(blue, alpha)),
            f32::from(alpha) / 255.0,
        ]);
    }
    let image = image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_raw(
        frame.width,
        frame.height,
        pixels,
    )
    .ok_or_else(|| ImageError("No se pudo preparar la imagen OpenEXR.".into()))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    image::DynamicImage::ImageRgba32F(image)
        .write_to(&mut output, image::ImageFormat::OpenExr)
        .map_err(|error| ImageError(error.to_string()))?;
    output.sync_all()?;
    Ok(())
}

fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        0
    } else {
        ((u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8
    }
}

fn flatten(channel: u8, alpha: u8) -> u8 {
    (u16::from(channel) + 255 - u16::from(alpha)).min(255) as u8
}

fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
