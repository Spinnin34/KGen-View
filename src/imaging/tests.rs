use super::*;
fn apply_orientation(
    mut frame: Frame,
    orientation: u32,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    match orientation {
        2 => flip_pixels_horizontal_cancellable(
            frame.pixels.try_make_mut()?,
            frame.width as usize,
            frame.height as usize,
            should_cancel,
        )?,
        3 => reverse_pixels_180_cancellable(frame.pixels.try_make_mut()?, should_cancel)?,
        4 => flip_pixels_vertical_cancellable(
            frame.pixels.try_make_mut()?,
            frame.width as usize,
            frame.height as usize,
            should_cancel,
        )?,
        5..=8 => return transform_frame_cancellable(&frame, orientation, should_cancel),
        _ => {}
    }
    Ok(frame)
}

fn reverse_pixels_180_cancellable(
    pixels: &mut [u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let pixel_count = pixels.len() / 4;
    for index in 0..pixel_count / 2 {
        if index % 65_536 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let left = index * 4;
        let right = (pixel_count - 1 - index) * 4;
        let (before_right, right_pixel) = pixels.split_at_mut(right);
        before_right[left..left + 4].swap_with_slice(&mut right_pixel[..4]);
    }
    Ok(())
}

fn flip_pixels_horizontal_cancellable(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let row_len = width * 4;
    for y in 0..height {
        if y % 32 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let row_start = y * row_len;
        let row = &mut pixels[row_start..row_start + row_len];
        for x in 0..width / 2 {
            let left_idx = x * 4;
            let right_idx = (width - 1 - x) * 4;
            for c in 0..4 {
                row.swap(left_idx + c, right_idx + c);
            }
        }
    }
    Ok(())
}

fn flip_pixels_vertical_cancellable(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<()> {
    let row_len = width * 4;
    for y in 0..height / 2 {
        if y % 32 == 0 && should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let other = height - 1 - y;
        let top_start = y * row_len;
        let bottom_start = other * row_len;
        let (top_half, bottom_half) = pixels.split_at_mut(bottom_start);
        top_half[top_start..top_start + row_len].swap_with_slice(&mut bottom_half[..row_len]);
    }
    Ok(())
}

fn transform_frame(frame: &Frame, orientation: u32) -> Frame {
    transform_frame_cancellable(frame, orientation, &|| false)
        .expect("uncancellable frame transform")
}

fn transform_frame_cancellable(
    frame: &Frame,
    orientation: u32,
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Frame> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let swaps_dimensions = matches!(orientation, 5..=8);
    let (new_width, new_height) = if swaps_dimensions {
        (height, width)
    } else {
        (width, height)
    };
    let mut pixels = vec![0; frame.pixels.len()];

    // Tiling keeps both source reads and rotated destination writes near each
    // other in memory. A single pass also avoids the intermediate full-size
    // frames previously allocated for compound EXIF orientations.
    const TILE: usize = 32;
    let reverse_y = matches!(orientation, 6 | 7);
    for tile_x in (0..width).step_by(TILE) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let x_end = (tile_x + TILE).min(width);
        for tile_y in (0..height).step_by(TILE) {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let y_end = (tile_y + TILE).min(height);
            for x in tile_x..x_end {
                if reverse_y {
                    for y in (tile_y..y_end).rev() {
                        copy_oriented_pixel(
                            frame,
                            &mut pixels,
                            width,
                            new_width,
                            x,
                            y,
                            orientation,
                        );
                    }
                } else {
                    for y in tile_y..y_end {
                        copy_oriented_pixel(
                            frame,
                            &mut pixels,
                            width,
                            new_width,
                            x,
                            y,
                            orientation,
                        );
                    }
                }
            }
        }
    }

    Ok(Frame {
        width: new_width as u32,
        height: new_height as u32,
        pixels: PixelBuffer::from_vec(pixels)?,
        delay_ms: frame.delay_ms,
    })
}

fn copy_oriented_pixel(
    source: &Frame,
    destination: &mut [u8],
    source_width: usize,
    destination_width: usize,
    x: usize,
    y: usize,
    orientation: u32,
) {
    let source_offset = (y * source_width + x) * 4;
    let (target_x, target_y) = match orientation {
        5 => (y, x),
        6 => (source.height as usize - 1 - y, x),
        7 => (source.height as usize - 1 - y, source_width - 1 - x),
        8 => (y, source_width - 1 - x),
        _ => unreachable!("only axis-swapping orientations use this path"),
    };
    let destination_offset = (target_y * destination_width + target_x) * 4;
    destination[destination_offset..destination_offset + 4]
        .copy_from_slice(&source.pixels[source_offset..source_offset + 4]);
}

fn clone_bytes_cancellable(
    source: &[u8],
    should_cancel: &impl Fn() -> bool,
) -> ImageResult<Vec<u8>> {
    let mut copy = vec![0; source.len()];
    for start in (0..source.len()).step_by(MAX_COPY_CHUNK_BYTES) {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let end = (start + MAX_COPY_CHUNK_BYTES).min(source.len());
        copy[start..end].copy_from_slice(&source[start..end]);
    }
    Ok(copy)
}

fn rotate(frame: &Frame, clockwise: bool) -> Frame {
    transform_frame(frame, if clockwise { 6 } else { 8 })
}

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

#[test]
fn wic_orientation_matches_all_eight_exif_transforms() {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let factory = ImageDocument::create_wic_factory().unwrap();
    let pixels: Vec<u8> = (0u8..6).flat_map(|v| [v, v, v, 255]).collect();
    let frame = Frame {
        width: 3,
        height: 2,
        pixels: PixelBuffer::from_vec(pixels).unwrap(),
        delay_ms: 100,
    };
    for orientation in 1..=8 {
        let bitmap = unsafe {
            factory
                .CreateBitmapFromMemory(3, 2, &GUID_WICPixelFormat32bppPBGRA, 12, &frame.pixels)
                .unwrap()
        };
        let source: IWICBitmapSource = bitmap.cast().unwrap();
        let expected = apply_orientation(frame.clone(), orientation, &|| false).unwrap();
        let source = prepare_wic_view(
            &factory,
            source,
            orientation,
            (expected.width, expected.height),
            &DecodeRequest::original(0),
        )
        .unwrap();
        let mut output = vec![0; expected.pixels.len()];
        unsafe {
            source
                .CopyPixels(std::ptr::null(), expected.width * 4, &mut output)
                .unwrap();
        }
        assert_eq!(
            output.as_slice(),
            expected.pixels.as_slice(),
            "orientation {orientation}"
        );
    }
    if initialized {
        unsafe {
            CoUninitialize();
        }
    }
}

#[test]
fn decode_cancels_before_opening_a_stale_request() {
    let result = ImageDocument::decode_cancellable(
        Path::new("stale-navigation-request-that-does-not-exist.png"),
        &|| true,
    );
    assert_eq!(
        result.expect_err("cancelled request").to_string(),
        "Carga cancelada."
    );
}

#[test]
fn large_buffer_copy_cancels_between_chunks() {
    let source = vec![7; MAX_COPY_CHUNK_BYTES * 3];
    let checks = std::sync::atomic::AtomicUsize::new(0);
    let result = clone_bytes_cancellable(&source, &|| {
        checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
    });
    assert_eq!(
        result.expect_err("cancelled chunked copy").to_string(),
        "Carga cancelada."
    );
}

#[test]
fn rotated_frame_cancels_between_tiles() {
    let frame = Frame {
        width: 64,
        height: 64,
        pixels: PixelBuffer::from_vec(vec![0; 64 * 64 * 4]).unwrap(),
        delay_ms: 100,
    };
    let checks = std::sync::atomic::AtomicUsize::new(0);
    let result = transform_frame_cancellable(&frame, 6, &|| {
        checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
    });
    assert_eq!(
        result.expect_err("cancelled tiled transform").to_string(),
        "Carga cancelada."
    );
}

fn sample() -> Frame {
    Frame {
        width: 2,
        height: 1,
        pixels: PixelBuffer::from_vec(vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
        delay_ms: 100,
    }
}

#[test]
fn rotate_preserves_pixels() {
    let rotated = rotate(&sample(), true);
    assert_eq!((rotated.width, rotated.height), (1, 2));
    assert_eq!(rotated.pixels, sample().pixels);
}

fn sample_document() -> ImageDocument {
    raster_document(
        Path::new("sample.png"),
        sample(),
        (2, 1),
        1,
        0,
        "PNG",
        &DecodeRequest::original(0),
    )
}
fn rendered(document: &ImageDocument) -> Frame {
    let mut request = DecodeRequest::original(0);
    request.operations = document.edits.operations.clone();
    render_raster_view(
        document.current_frame(),
        document.original_size,
        &request,
        &|| false,
    )
    .unwrap()
}
#[test]
fn editing_history_restores_and_reapplies_pixels() {
    let mut document = sample_document();
    document.flip_horizontal();
    assert_eq!(&rendered(&document).pixels[..], &[5, 6, 7, 8, 1, 2, 3, 4]);
    assert!(document.undo());
    assert_eq!(&rendered(&document).pixels[..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(document.redo());
    assert_eq!(&rendered(&document).pixels[..], &[5, 6, 7, 8, 1, 2, 3, 4]);
    document.rotate_right();
    assert_eq!(document.view_dimensions(), (1, 2));
    assert!(document.crop(0, 0, 1, 1));
    assert_eq!(document.view_dimensions(), (1, 1));
    assert!(!document.crop(1, 0, 1, 1));
    assert!(document.undo());
    assert_eq!(document.view_dimensions(), (1, 2));
}

#[test]
fn export_snapshot_shares_pixels_and_freezes_edit_history() {
    let mut document = sample_document();
    let snapshot = document.export_snapshot();
    document.flip_horizontal();
    assert!(
        document
            .current_frame()
            .pixels
            .shares_storage(&snapshot.current_frame().pixels)
    );
    assert_eq!(&rendered(&snapshot).pixels[..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&rendered(&document).pixels[..], &[5, 6, 7, 8, 1, 2, 3, 4]);
    assert_eq!(snapshot.revision, 0);
    assert_eq!(document.revision, 1);
}

#[test]
fn wic_round_trip_png() {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let path = std::env::temp_dir().join(format!("kgen-view-test-{}.png", std::process::id()));
    let frame = Frame {
        width: 2,
        height: 1,
        pixels: PixelBuffer::from_vec(vec![0, 0, 255, 255, 0, 255, 0, 255]).unwrap(),
        delay_ms: 100,
    };
    let mut document = ImageDocument {
        path: path.clone(),
        frames: vec![frame],
        frame_index: 0,
        original_size: (2, 1),
        resolution_dpi: (96.0, 96.0),
        file_size: 0,
        format_name: "PNG".into(),
        metadata: Vec::new(),
        metadata_dirty: false,
        dirty: true,
        id: DocumentId::new(),
        revision: 0,
        is_preview: false,
        rendered_region: (0, 0, 2, 1),
        source_frame_count: 1,
        source_lease: None,
        source: None,
        edits: EditHistory::default(),
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
    let converted_path = path.with_file_name(format!(
        "kgen-view-converted-test-{}.png",
        std::process::id()
    ));
    let png_format = ExportFormat {
        container: windows::Win32::Graphics::Imaging::GUID_ContainerFormatPng,
        name: "PNG".into(),
        extensions: vec!["png".into()],
    };
    document
        .export_snapshot()
        .export_copy_as(&converted_path, &png_format)
        .expect("convert PNG copy with WIC");
    let converted = ImageDocument::decode(&converted_path).expect("decode converted PNG");
    assert_eq!(
        converted.current_frame().pixels.as_slice(),
        document.current_frame().pixels.as_slice()
    );
    let _ = std::fs::remove_file(converted_path);
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
    let decoded = decode_gif(&path, &|| false, &DecodeRequest::original(0)).expect("decode GIF");
    assert_eq!(decoded.frame_count(), 2);
    assert_eq!(decoded.frames[0].delay_ms, 40);
    assert_eq!(decoded.frames.len(), 1);
    let second = decode_gif(&path, &|| false, &DecodeRequest::original(1)).unwrap();
    assert_eq!(second.current_frame().delay_ms, 70);
    assert_eq!(&second.current_frame().pixels[0..4], &[0, 0, 255, 255]);
    gif_source::clear();
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
