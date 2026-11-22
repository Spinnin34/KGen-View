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
                EditOperation::Canvas { size, offset, fill } => {
                    canvas(&frame, size, offset, fill, cancel)?
                }
                EditOperation::Resize { size, filter } => {
                    resize(&frame, size, filter, factory, cancel)?
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

    pub fn histogram(&self, cancel: &impl Fn() -> bool) -> ImageResult<Histogram> {
        let rendered = self.render_view(DecodeRequest::original(self.frame_index), cancel, None)?;
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
    fill: [u8; 4],
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    let mut result = new_frame(frame, size)?;
    let stride = size.0 as usize * 4;
    let (x, y) = (i64::from(offset.0), i64::from(offset.1));
    let left = x.clamp(0, i64::from(size.0));
    let right = (x + i64::from(frame.width)).clamp(left, i64::from(size.0));
    for (row, output) in result
        .pixels
        .try_make_mut()?
        .chunks_exact_mut(stride)
        .enumerate()
    {
        if row % 32 == 0 && cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        if fill != [0; 4] {
            for pixel in output.as_chunks_mut::<4>().0 {
                *pixel = fill;
            }
        }
        let source_y = row as i64 - y;
        if left < right && (0..i64::from(frame.height)).contains(&source_y) {
            let start = (source_y * i64::from(frame.width) + left - x) as usize * 4;
            let len = (right - left) as usize * 4;
            output[left as usize * 4..right as usize * 4]
                .copy_from_slice(&frame.pixels[start..start + len]);
        }
    }
    Ok(result)
}

fn resize(
    frame: &Frame,
    size: (u32, u32),
    filter: ResampleFilter,
    factory: Option<&IWICImagingFactory>,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    if size == (frame.width, frame.height) {
        return Ok(frame.clone());
    }
    let mut result = new_frame(frame, size)?;
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
    decoding::copy_pixels_cancellable(
        &scaler,
        size.0,
        size.1,
        size.0 * 4,
        result.pixels.try_make_mut()?,
        cancel,
    )?;
    Ok(result)
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
