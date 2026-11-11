use super::*;
use windows::Win32::Graphics::Imaging::*;

/// Ask the codec to reduce during decode, before ICC conversion and raster scaling.
pub(super) fn native_thumbnail_source(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
    original: (u32, u32),
    orientation: u32,
    request: &DecodeRequest,
    cancel: &impl Fn() -> bool,
) -> ImageResult<Option<IWICBitmapSource>> {
    if !request.allow_downsample {
        return Ok(None);
    }
    let _span = crate::diagnostics::Span::new("native_thumbnail");
    let Ok(transform) = frame.cast::<IWICBitmapSourceTransform>() else {
        return Ok(None);
    };
    let layout = request.layout(original);
    let native = if matches!(orientation, 5..=8) {
        (original.1, original.0)
    } else {
        original
    };
    let (l, t, r, b) = layout.region;
    let scale = (f64::from(layout.size.0) / f64::from(r - l))
        .min(f64::from(layout.size.1) / f64::from(b - t))
        .min(1.0);
    let (mut width, mut height) = (
        (f64::from(native.0) * scale).ceil().max(1.0) as u32,
        (f64::from(native.1) * scale).ceil().max(1.0) as u32,
    );
    if unsafe { transform.GetClosestSize(&mut width, &mut height) }.is_err() {
        return Ok(None);
    }
    if (width, height) == native {
        return Ok(None);
    }
    let mut format = GUID_WICPixelFormat32bppPBGRA;
    if unsafe { transform.GetClosestPixelFormat(&mut format) }.is_err() {
        return Ok(None);
    }
    let channels = if format == GUID_WICPixelFormat24bppBGR || format == GUID_WICPixelFormat24bppRGB
    {
        3
    } else if format == GUID_WICPixelFormat32bppBGRA
        || format == GUID_WICPixelFormat32bppPBGRA
        || format == GUID_WICPixelFormat32bppBGR
    {
        4
    } else {
        return Ok(None);
    };
    let bytes = width as usize * height as usize * channels as usize;
    if bytes > crate::resources::PREVIEW_LIMIT * 2 {
        return Ok(None);
    }
    if cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let mut pixels = PixelBuffer::zeroed(bytes)?;
    if unsafe {
        transform.CopyPixels(
            std::ptr::null(),
            width,
            height,
            &format,
            WICBitmapTransformRotate0,
            width * channels,
            pixels.try_make_mut()?,
        )
    }
    .is_err()
    {
        return Ok(None);
    }
    if cancel() {
        return Err(ImageError("Carga cancelada.".into()));
    }
    let bitmap = unsafe {
        factory.CreateBitmapFromMemory(width, height, &format, width * channels, &pixels)?
    };
    Ok(Some(bitmap.cast()?))
}

pub(super) fn prepare_wic_view(
    factory: &IWICImagingFactory,
    mut source: IWICBitmapSource,
    orientation: u32,
    original: (u32, u32),
    request: &DecodeRequest,
) -> ImageResult<IWICBitmapSource> {
    source = orient(factory, source, orientation)?;
    let mut decoded_width = 0;
    let mut decoded_height = 0;
    unsafe {
        source.GetSize(&mut decoded_width, &mut decoded_height)?;
    }
    let view = ViewTransform::from_operations(original, &request.operations);
    let bounds = view.source_bounds();
    if bounds != (0, 0, original.0, original.1) {
        source = clip(
            factory,
            source,
            scaled_rect(bounds, original, (decoded_width, decoded_height)),
        )?;
    }
    source = orient(factory, source, view.orientation())?;
    let size = (view.width, view.height);
    let layout = request.layout(original);
    let mut width = 0;
    let mut height = 0;
    unsafe {
        source.GetSize(&mut width, &mut height)?;
    }
    if layout.region != (0, 0, size.0, size.1) {
        let region = scaled_rect(layout.region, size, (width, height));
        source = clip(factory, source, region)?;
        width = region.2 - region.0;
        height = region.3 - region.1;
    }
    if layout.size != (width, height) {
        let scaler = unsafe { factory.CreateBitmapScaler()? };
        unsafe {
            scaler.Initialize(
                &source,
                layout.size.0,
                layout.size.1,
                WICBitmapInterpolationModeFant,
            )?;
        }
        source = scaler.cast()?;
    }
    Ok(source)
}

fn scaled_rect(
    region: (u32, u32, u32, u32),
    original: (u32, u32),
    decoded: (u32, u32),
) -> (u32, u32, u32, u32) {
    let (l, t, r, b) = region;
    let left = (u64::from(l) * u64::from(decoded.0) / u64::from(original.0)) as u32;
    let top = (u64::from(t) * u64::from(decoded.1) / u64::from(original.1)) as u32;
    let right = (u64::from(r) * u64::from(decoded.0)).div_ceil(u64::from(original.0)) as u32;
    let bottom = (u64::from(b) * u64::from(decoded.1)).div_ceil(u64::from(original.1)) as u32;
    (
        left.min(decoded.0 - 1),
        top.min(decoded.1 - 1),
        right.min(decoded.0).max(left + 1),
        bottom.min(decoded.1).max(top + 1),
    )
}

fn rotate(
    factory: &IWICImagingFactory,
    source: IWICBitmapSource,
    transform: WICBitmapTransformOptions,
) -> ImageResult<IWICBitmapSource> {
    let rotator = unsafe { factory.CreateBitmapFlipRotator()? };
    unsafe {
        rotator.Initialize(&source, transform)?;
    }
    Ok(rotator.cast()?)
}

fn clip(
    factory: &IWICImagingFactory,
    source: IWICBitmapSource,
    region: (u32, u32, u32, u32),
) -> ImageResult<IWICBitmapSource> {
    let clipper = unsafe { factory.CreateBitmapClipper()? };
    let (l, t, r, b) = region;
    unsafe {
        clipper.Initialize(
            &source,
            &WICRect {
                X: l as i32,
                Y: t as i32,
                Width: (r - l) as i32,
                Height: (b - t) as i32,
            },
        )?;
    }
    Ok(clipper.cast()?)
}

fn orient(
    factory: &IWICImagingFactory,
    mut source: IWICBitmapSource,
    orientation: u32,
) -> ImageResult<IWICBitmapSource> {
    let transform = match orientation {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 | 6 => WICBitmapTransformRotate90,
        7 | 8 => WICBitmapTransformRotate270,
        _ => WICBitmapTransformRotate0,
    };
    if transform != WICBitmapTransformRotate0 {
        source = rotate(factory, source, transform)?;
    }
    if matches!(orientation, 5 | 7) {
        source = rotate(factory, source, WICBitmapTransformFlipHorizontal)?;
    }
    Ok(source)
}
