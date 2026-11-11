use super::*;

pub(super) fn raster_document(
    path: &Path,
    frame: Frame,
    original: (u32, u32),
    count: usize,
    page: usize,
    format: &str,
    request: &DecodeRequest,
) -> ImageDocument {
    let layout = request.layout(original);
    ImageDocument {
        id: DocumentId::new(),
        revision: 0,
        path: path.to_path_buf(),
        frames: vec![frame],
        frame_index: page,
        original_size: original,
        resolution_dpi: (96.0, 96.0),
        file_size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        format_name: format.into(),
        metadata: Vec::new(),
        metadata_dirty: false,
        dirty: false,
        is_preview: layout.size != original
            || !request.operations.is_empty()
            || request.region.is_some(),
        rendered_region: layout.region,
        source_frame_count: count,
        source_lease: None,
        source: ImageSource::open(path).ok(),
        edits: EditHistory::from_operations(request.operations.clone()),
    }
}

pub(super) fn render_raster_view(
    frame: &Frame,
    original: (u32, u32),
    request: &DecodeRequest,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    render_raster_region(frame, original, (0, 0), request, cancel)
}

pub(super) fn render_raster_region(
    frame: &Frame,
    original: (u32, u32),
    origin: (u32, u32),
    request: &DecodeRequest,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    let layout = request.layout(original);
    let (l, t, r, b) = layout.region;
    let (width, height) = layout.size;
    if origin == (0, 0)
        && layout.region == (0, 0, frame.width, frame.height)
        && layout.size == (frame.width, frame.height)
        && request.operations.is_empty()
    {
        return Ok(frame.clone());
    }
    let transform = ViewTransform::from_operations(original, &request.operations);
    let mut pixels = PixelBuffer::zeroed(width as usize * height as usize * 4)?;
    let data = pixels.try_make_mut()?;
    for y in 0..height {
        if y % 32 == 0 && cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let vy = t + ((u64::from(y) * 2 + 1) * u64::from(b - t) / u64::from(height) / 2) as u32;
        for x in 0..width {
            let vx = l + ((u64::from(x) * 2 + 1) * u64::from(r - l) / u64::from(width) / 2) as u32;
            let (sx, sy) = transform.source_pixel(vx.min(r - 1), vy.min(b - 1));
            let sx = sx.saturating_sub(origin.0).min(frame.width - 1);
            let sy = sy.saturating_sub(origin.1).min(frame.height - 1);
            let source = (sy as usize * frame.width as usize + sx as usize) * 4;
            let target = (y as usize * width as usize + x as usize) * 4;
            data[target..target + 4].copy_from_slice(&frame.pixels[source..source + 4]);
        }
    }
    Ok(Frame {
        width,
        height,
        pixels,
        delay_ms: frame.delay_ms,
    })
}

pub(super) fn source_region(original: (u32, u32), request: &DecodeRequest) -> (u32, u32, u32, u32) {
    let transform = ViewTransform::from_operations(original, &request.operations);
    let (l, t, r, b) = request.layout(original).region;
    let points = [
        transform.source_pixel(l, t),
        transform.source_pixel(r - 1, t),
        transform.source_pixel(l, b - 1),
        transform.source_pixel(r - 1, b - 1),
    ];
    (
        points.iter().map(|p| p.0).min().unwrap(),
        points.iter().map(|p| p.1).min().unwrap(),
        points.iter().map(|p| p.0).max().unwrap() + 1,
        points.iter().map(|p| p.1).max().unwrap() + 1,
    )
}

pub(super) fn clear_rect_cancellable(
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
