use super::*;
use windows::Win32::Graphics::Imaging::{
    WICBitmapInterpolationModeFant, WICBitmapInterpolationModeHighQualityCubic,
    WICBitmapInterpolationModeLinear, WICBitmapInterpolationModeNearestNeighbor,
};

#[derive(Clone, Debug)]
pub(super) struct RasterCache {
    operations: Vec<EditOperation>,
    page: usize,
    frame: Frame,
}

pub fn editable_size((width, height): (u32, u32)) -> bool {
    validate_dimensions(width, height).is_ok()
        && u64::from(width) * u64::from(height) * 4 <= MAX_DECODE_BYTES
}

impl ImageDocument {
    pub(super) fn render_raster_edits(
        &self,
        request: &DecodeRequest,
        cancel: &impl Fn() -> bool,
        factory: Option<&IWICImagingFactory>,
    ) -> ImageResult<Self> {
        let operations = &request.operations;
        let first_raster = operations
            .iter()
            .position(EditOperation::is_raster)
            .unwrap();
        let cache = self.raster_cache.as_ref().filter(|cache| {
            cache.page == request.page && operations.starts_with(&cache.operations)
        });
        let (mut rendered, mut frame, mut start) = if let Some(cache) = cache {
            let mut rendered = self.clone();
            rendered.frames.clear();
            (rendered, cache.frame.clone(), cache.operations.len())
        } else {
            let mut base = DecodeRequest::original(request.page);
            base.operations
                .extend_from_slice(&operations[..first_raster]);
            let mut rendered = Self::decode_request(&self.path, cancel, factory, &base)?;
            let frame = rendered.frames.pop().expect("decoded frame");
            (rendered, frame, first_raster)
        };

        for (index, &operation) in operations.iter().enumerate().skip(start) {
            if !operation.is_raster() {
                continue;
            }
            if cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            if index != start {
                let mut geometry = DecodeRequest::original(request.page);
                geometry
                    .operations
                    .extend_from_slice(&operations[start..index]);
                frame = render_raster_view(&frame, (frame.width, frame.height), &geometry, cancel)?;
            }
            frame = match operation {
                EditOperation::Canvas {
                    size,
                    offset,
                    inset,
                    paint,
                } => canvas(&frame, size, offset, inset, paint, cancel)?,
                EditOperation::Resize { size, options } => {
                    resize(&frame, size, options, factory, cancel)?
                }
                _ => unreachable!(),
            };
            start = index + 1;
        }
        let mut view = request.clone();
        view.operations = operations[start..].to_vec();
        let size = (frame.width, frame.height);
        let layout = view.layout(size);
        let bytes = u64::from(layout.size.0) * u64::from(layout.size.1) * 4;
        if bytes > request.max_bytes as u64 {
            return Err(ImageError("Imagen demasiado grande.".into()));
        }
        rendered
            .frames
            .push(render_raster_view(&frame, size, &view, cancel)?);
        rendered.raster_cache = Some(RasterCache {
            operations: operations[..start].to_vec(),
            page: request.page,
            frame,
        });
        rendered.is_preview = request.allow_downsample || request.region.is_some();
        rendered.rendered_region = layout.region;
        Ok(rendered)
    }

    pub fn histogram(
        &self,
        region: Option<(u32, u32, u32, u32)>,
        cancel: &impl Fn() -> bool,
    ) -> ImageResult<Histogram> {
        let mut request = DecodeRequest::original(self.frame_index);
        request.region = region;
        let rendered = self.render_view(request, cancel, None)?;
        Histogram::from_frame(rendered.current_frame(), cancel)
    }
}

fn new_frame(frame: &Frame, (width, height): (u32, u32)) -> ImageResult<Frame> {
    if !editable_size((width, height)) {
        return Err(ImageError(
            "Imagen demasiado grande o dimensiones no válidas.".into(),
        ));
    }
    Ok(Frame {
        width,
        height,
        pixels: PixelBuffer::zeroed(width as usize * height as usize * 4)?,
        delay_ms: frame.delay_ms,
    })
}

fn canvas(
    frame: &Frame,
    size: (u32, u32),
    offset: (i32, i32),
    inset: [u32; 4],
    paint: CanvasPaint,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    let mut result = new_frame(frame, size)?;
    let stride = size.0 as usize * 4;
    let (x, y) = (i64::from(offset.0), i64::from(offset.1));
    let left = (x + i64::from(inset[0])).clamp(0, i64::from(size.0));
    let right =
        (x + i64::from(frame.width.saturating_sub(inset[2]))).clamp(left, i64::from(size.0));
    let bottom = i64::from(frame.height.saturating_sub(inset[3]));
    let gamma = std::array::from_fn(|i| {
        ((i as f32 / 255.0).powf(100.0 / f32::from(paint.gamma.max(1))) * 255.0).round() as u8
    });
    let background = paint
        .blur
        .map(|(strength, flip)| (blurred_thumbnail(frame, strength), flip));
    let solid = gamma_pixel(paint.color, &gamma);

    for (row, output) in result
        .pixels
        .try_make_mut()?
        .chunks_exact_mut(stride)
        .enumerate()
    {
        if row % 32 == 0 && cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let source_y = row as i64 - y;
        let has_source = left < right && (i64::from(inset[1])..bottom).contains(&source_y);
        let (start_x, end_x) = if has_source {
            (left as usize, right as usize)
        } else {
            (0, 0)
        };
        if let Some((image, flip)) = &background {
            for (column, pixel) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                if !(start_x..end_x).contains(&column) {
                    let x = if *flip {
                        size.0 - 1 - column as u32
                    } else {
                        column as u32
                    };
                    let sample = sample_bilinear(image, size, x, row as u32);
                    *pixel = gamma_pixel(sample, &gamma);
                }
            }
        } else if solid != [0; 4] {
            let pixels = output.as_chunks_mut::<4>().0;
            pixels[..start_x].fill(solid);
            pixels[end_x..].fill(solid);
        }
        if has_source {
            let start = (source_y * i64::from(frame.width) + left - x) as usize * 4;
            let len = (end_x - start_x) * 4;
            output[start_x * 4..end_x * 4].copy_from_slice(&frame.pixels[start..start + len]);
        }
    }
    Ok(result)
}

fn gamma_pixel(mut pixel: [u8; 4], gamma: &[u8; 256]) -> [u8; 4] {
    let Some(alpha) = std::num::NonZeroU16::new(u16::from(pixel[3])) else {
        return pixel;
    };
    let alpha = alpha.get();
    for channel in &mut pixel[..3] {
        let straight = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255);
        *channel = ((u16::from(gamma[straight as usize]) * alpha + 127) / 255) as u8;
    }
    pixel
}

fn blurred_thumbnail(frame: &Frame, strength: u8) -> image::RgbaImage {
    let (w, h) = fitted_size((frame.width, frame.height), (320, 320), 320 * 320 * 4);
    let thumbnail = image::RgbaImage::from_fn(w, h, |x, y| {
        let x = (u64::from(x) * u64::from(frame.width) / u64::from(w)) as usize;
        let y = (u64::from(y) * u64::from(frame.height) / u64::from(h)) as usize;
        let index = (y * frame.width as usize + x) * 4;
        image::Rgba(frame.pixels[index..index + 4].try_into().unwrap())
    });
    let sigma = (f32::from(strength) * w.max(h) as f32 / 1600.0).max(0.3);
    image::imageops::blur(&thumbnail, sigma)
}

fn sample_bilinear(image: &image::RgbaImage, size: (u32, u32), x: u32, y: u32) -> [u8; 4] {
    let fx =
        f64::from(x) * f64::from(image.width() - 1) / f64::from(size.0.saturating_sub(1).max(1));
    let fy =
        f64::from(y) * f64::from(image.height() - 1) / f64::from(size.1.saturating_sub(1).max(1));
    let (ix, iy) = (fx as u32, fy as u32);
    let (jx, jy) = (
        (ix + 1).min(image.width() - 1),
        (iy + 1).min(image.height() - 1),
    );
    let (a, b) = (fx.fract(), fy.fract());
    let [tl, tr, bl, br] = [
        image.get_pixel(ix, iy),
        image.get_pixel(jx, iy),
        image.get_pixel(ix, jy),
        image.get_pixel(jx, jy),
    ];
    std::array::from_fn(|c| {
        (f64::from(tl[c]) * (1.0 - a) * (1.0 - b)
            + f64::from(tr[c]) * a * (1.0 - b)
            + f64::from(bl[c]) * (1.0 - a) * b
            + f64::from(br[c]) * a * b)
            .round() as u8
    })
}

fn resize(
    frame: &Frame,
    size: (u32, u32),
    options: ResizeOptions,
    factory: Option<&IWICImagingFactory>,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    if size == (frame.width, frame.height) {
        return Ok(frame.clone());
    }
    if !editable_size(size) {
        return Err(ImageError(
            "Imagen demasiado grande o dimensiones no válidas.".into(),
        ));
    }
    let mut result = if options.linear_light
        || matches!(
            options.filter,
            ResampleFilter::Lanczos | ResampleFilter::Gaussian
        ) {
        Frame {
            width: size.0,
            height: size.1,
            delay_ms: frame.delay_ms,
            pixels: resize_precise(frame, size, options, cancel)?,
        }
    } else {
        let mut result = new_frame(frame, size)?;
        resize_wic(
            frame,
            size,
            options.filter,
            factory,
            result.pixels.try_make_mut()?,
            cancel,
        )?;
        result
    };
    if options.sharpen {
        sharpen(&mut result, cancel)?;
    }
    Ok(result)
}

fn resize_wic(
    frame: &Frame,
    size: (u32, u32),
    filter: ResampleFilter,
    factory: Option<&IWICImagingFactory>,
    data: &mut [u8],
    cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let owned_factory;
    let factory = if let Some(factory) = factory {
        factory
    } else {
        owned_factory = ImageDocument::create_wic_factory()?;
        &owned_factory
    };
    let mode = match filter {
        ResampleFilter::Nearest => WICBitmapInterpolationModeNearestNeighbor,
        ResampleFilter::Linear => WICBitmapInterpolationModeLinear,
        ResampleFilter::Cubic => WICBitmapInterpolationModeHighQualityCubic,
        ResampleFilter::Area => WICBitmapInterpolationModeFant,
        ResampleFilter::Lanczos | ResampleFilter::Gaussian => unreachable!(),
    };
    let scaler = unsafe {
        let source = factory.CreateBitmapFromMemory(
            frame.width,
            frame.height,
            &GUID_WICPixelFormat32bppPBGRA,
            frame.width * 4,
            &frame.pixels,
        )?;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&source, size.0, size.1, mode)?;
        scaler.cast()?
    };
    decoding::copy_pixels_cancellable(&scaler, size.0, size.1, size.0 * 4, data, cancel)
}

fn resize_precise(
    frame: &Frame,
    size: (u32, u32),
    options: ResizeOptions,
    cancel: &impl Fn() -> bool,
) -> ImageResult<PixelBuffer> {
    use image::{
        ImageBuffer, Rgba,
        imageops::{FilterType, resize},
    };
    let filter = match options.filter {
        ResampleFilter::Nearest => FilterType::Nearest,
        ResampleFilter::Linear | ResampleFilter::Area => FilterType::Triangle,
        ResampleFilter::Cubic => FilterType::CatmullRom,
        ResampleFilter::Lanczos => FilterType::Lanczos3,
        ResampleFilter::Gaussian => FilterType::Gaussian,
    };
    let source_pixels = frame.width as usize * frame.height as usize;
    let target_pixels = size.0 as usize * size.1 as usize;
    // The sampler first resizes vertically into a floating-point intermediate.
    let scratch = frame.width as usize * size.1 as usize * 16
        + if options.linear_light {
            source_pixels * 16 + target_pixels * 16
        } else {
            0
        };
    let _reservation = crate::resources::cpu_budget().reserve(scratch)?;
    if cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    if options.linear_light {
        let mut input = image::Rgba32FImage::new(frame.width, frame.height);
        let transfer = |value: f32| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        for (index, (pixel, out)) in frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(input.pixels_mut())
            .enumerate()
        {
            if index.is_multiple_of(1 << 18) && cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let a = f32::from(pixel[3]) / 255.0;
            if a > 0.0 {
                *out = Rgba([
                    transfer(f32::from(pixel[0]) / f32::from(pixel[3])) * a,
                    transfer(f32::from(pixel[1]) / f32::from(pixel[3])) * a,
                    transfer(f32::from(pixel[2]) / f32::from(pixel[3])) * a,
                    a,
                ]);
            }
        }
        let resized = resize(&input, size.0, size.1, filter);
        drop(input);
        let mut output = PixelBuffer::zeroed(target_pixels * 4)?;
        for (index, (pixel, out)) in resized
            .pixels()
            .zip(output.try_make_mut()?.as_chunks_mut::<4>().0)
            .enumerate()
        {
            if index.is_multiple_of(1 << 18) && cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let a = pixel[3].clamp(0.0, 1.0);
            if a > 0.0 {
                out[3] = (a * 255.0).round() as u8;
                for c in 0..3 {
                    let v = (pixel[c] / a).clamp(0.0, 1.0);
                    let v = if v <= 0.0031308 {
                        12.92 * v
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    };
                    out[c] = (v * f32::from(out[3])).round() as u8;
                }
            }
        }
        Ok(output)
    } else {
        let input = ImageBuffer::<Rgba<u8>, _>::from_raw(
            frame.width,
            frame.height,
            frame.pixels.as_slice(),
        )
        .expect("validated frame buffer");
        let reservation = crate::resources::cpu_budget().reserve(target_pixels * 4)?;
        let mut resized = resize(&input, size.0, size.1, filter).into_raw();
        for (index, pixel) in resized.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if index.is_multiple_of(1 << 18) && cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let alpha = pixel[3];
            for channel in &mut pixel[..3] {
                *channel = (*channel).min(alpha);
            }
        }
        Ok(PixelBuffer::with_reservation(resized, reservation)?)
    }
}

fn sharpen(frame: &mut Frame, cancel: &impl Fn() -> bool) -> ImageResult<()> {
    let source = frame.pixels.clone();
    let width = frame.width as usize;
    let height = frame.height as usize;
    let out = frame.pixels.try_make_mut()?.as_chunks_mut::<4>().0;
    let input = source.as_chunks::<4>().0;
    for (y, row) in out.chunks_exact_mut(width).enumerate() {
        if y.is_multiple_of(32) && cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        for (x, target) in row.iter_mut().enumerate() {
            let i = y * width + x;
            let neighbors = [
                y * width + x.saturating_sub(1),
                y * width + (x + 1).min(width - 1),
                y.saturating_sub(1) * width + x,
                (y + 1).min(height - 1) * width + x,
            ];
            for c in 0..3 {
                let center = i16::from(input[i][c]);
                let around: i16 = neighbors.iter().map(|&n| i16::from(input[n][c])).sum();
                target[c] =
                    (center + (center * 4 - around) / 4).clamp(0, i16::from(target[3])) as u8;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct Histogram {
    pub channels: [[u64; 256]; 4], // Red, green, blue, luminance.
    pub pixels: u64,
    pub visible: u64,
}

impl Histogram {
    fn from_frame(frame: &Frame, cancel: &impl Fn() -> bool) -> ImageResult<Self> {
        let mut histogram = Self {
            channels: [[0; 256]; 4],
            pixels: u64::from(frame.width) * u64::from(frame.height),
            visible: 0,
        };
        for chunk in frame.pixels.chunks(MAX_COPY_CHUNK_BYTES) {
            if cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            for &[blue, green, red, alpha] in chunk.as_chunks::<4>().0 {
                if alpha == 0 {
                    continue;
                }
                let straight = |value: u8| {
                    if alpha == 255 {
                        u32::from(value)
                    } else {
                        let alpha = u32::from(alpha);
                        ((u32::from(value) * 255 + alpha / 2) / alpha).min(255)
                    }
                };
                let (r, g, b) = (straight(red), straight(green), straight(blue));
                let luminance = (2126 * r + 7152 * g + 722 * b + 5000) / 10_000;
                histogram.channels[0][r as usize] += 1;
                histogram.channels[1][g as usize] += 1;
                histogram.channels[2][b as usize] += 1;
                histogram.channels[3][luminance as usize] += 1;
                histogram.visible += 1;
            }
        }
        Ok(histogram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn black_and_white() -> Frame {
        Frame {
            width: 2,
            height: 1,
            pixels: PixelBuffer::from_vec(vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap(),
            delay_ms: 100,
        }
    }

    #[test]
    fn blurred_border_leaves_original_pixels_untouched() {
        let original = black_and_white();
        let image = canvas(
            &original,
            (4, 1),
            (1, 0),
            [0; 4],
            CanvasPaint {
                color: [0; 4],
                blur: Some((15, true)),
                gamma: 200,
            },
            &|| false,
        )
        .unwrap();
        assert_eq!(&image.pixels[4..12], &original.pixels[..]);
        assert!(image.pixels[0] > image.pixels[12]);
        assert_eq!((image.pixels[3], image.pixels[15]), (255, 255));
    }

    #[test]
    fn gamma_correct_resizing_averages_in_linear_light() {
        let resized = resize(
            &black_and_white(),
            (1, 1),
            ResizeOptions {
                filter: ResampleFilter::Lanczos,
                linear_light: true,
                sharpen: false,
                dpi: None,
            },
            None,
            &|| false,
        )
        .unwrap();
        assert_eq!(&resized.pixels[..], &[188, 188, 188, 255]);
    }
}
