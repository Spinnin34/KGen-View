use super::*;

pub(super) fn decode_svg(
    path: &Path,
    should_cancel: &impl Fn() -> bool,
    request: &DecodeRequest,
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
    let _input_memory = crate::resources::cpu_budget().reserve(file_size as usize)?;
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
    let original = (width, height);
    let layout = request.layout(original);
    let (width, height) = layout.size;
    let pixel_memory =
        crate::resources::cpu_budget().reserve(width as usize * height as usize * 4)?;
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
        svg_view_transform(original, request),
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

    let pixels = PixelBuffer::with_reservation(pixels, pixel_memory)?;
    let frame = Frame {
        width,
        height,
        pixels,
        delay_ms: 100,
    };
    Ok(raster_document(path, frame, original, 1, 0, "SVG", request))
}

fn svg_view_transform(
    original: (u32, u32),
    request: &DecodeRequest,
) -> resvg::tiny_skia::Transform {
    let transform = ViewTransform::from_operations(original, &request.operations);
    let [xx, xy, yx, yy, ox, oy] = transform.inverse_matrix();
    let layout = request.layout(original);
    let (l, t, r, b) = layout.region;
    let sx = layout.size.0 as f32 / (r - l) as f32;
    let sy = layout.size.1 as f32 / (b - t) as f32;
    resvg::tiny_skia::Transform::from_row(
        xx * sx,
        xy * sy,
        yx * sx,
        yy * sy,
        (ox - l as f32) * sx,
        (oy - t as f32) * sy,
    )
}
