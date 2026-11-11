use super::*;
use windows::Win32::{
    Graphics::Imaging::*,
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

struct Com(bool);
impl Com {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn path(label: &str, extension: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "kgen-regression-{}-{}-{label}.{extension}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn fixture(path: &Path) -> ImageDocument {
    let mut pixels = PixelBuffer::zeroed(8 * 6 * 4).unwrap();
    for (index, pixel) in pixels
        .try_make_mut()
        .unwrap()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        pixel.copy_from_slice(&[(index * 3) as u8, (index * 5) as u8, (index * 2) as u8, 255]);
    }
    let frame = Frame {
        width: 8,
        height: 6,
        pixels,
        delay_ms: 100,
    };
    let mut document = raster_document(
        path,
        frame,
        (8, 6),
        1,
        0,
        "PNG",
        &DecodeRequest::original(0),
    );
    document.source = None;
    let png = ExportFormat {
        container: GUID_ContainerFormatPng,
        name: "PNG".into(),
        extensions: vec!["png".into()],
    };
    document.save_as_format(path, &png).unwrap();
    document
}

#[test]
fn progressive_export_preserves_native_pixels_after_rotation_and_crop() {
    let _com = Com::new();
    let source = path("source", "png");
    let output = path("edited", "png");
    let original = fixture(&source);
    let mut preview =
        ImageDocument::decode_request(&source, &|| false, None, &DecodeRequest::preview((2, 2)))
            .unwrap();
    assert!(preview.is_preview);
    assert!(preview.current_frame().pixels.len() <= 16);
    assert_eq!(preview.view_dimensions(), (8, 6));
    preview.rotate_right();
    preview.crop(1, 2, 5, 7);
    let png = ExportFormat {
        container: GUID_ContainerFormatPng,
        name: "PNG".into(),
        extensions: vec!["png".into()],
    };
    preview.export_copy_as(&output, &png).unwrap();
    let exported =
        ImageDocument::decode_request(&output, &|| false, None, &DecodeRequest::original(0))
            .unwrap();
    assert_eq!(exported.view_dimensions(), (4, 5));
    let mut request = DecodeRequest::original(0);
    request.operations = preview.edits.operations.clone();
    let expected =
        render_raster_view(original.current_frame(), (8, 6), &request, &|| false).unwrap();
    assert_eq!(exported.current_frame().pixels, expected.pixels);
    assert_eq!(preview.path, source.canonicalize().unwrap());
    fs::remove_file(source).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
fn visible_region_contains_native_detail_and_preserves_original_dimensions() {
    let _com = Com::new();
    let source = path("region", "png");
    let original = fixture(&source);
    let mut request = DecodeRequest::preview((4, 3));
    request.region = Some((2, 1, 6, 4));
    let region = ImageDocument::decode_request(&source, &|| false, None, &request).unwrap();
    assert_eq!(region.original_size, (8, 6));
    assert_eq!(region.rendered_region, (2, 1, 6, 4));
    assert_eq!(
        (region.current_frame().width, region.current_frame().height),
        (4, 3)
    );
    assert_eq!(
        &region.current_frame().pixels[..4],
        &original.current_frame().pixels[(8 + 2) * 4..(8 + 2) * 4 + 4]
    );
    fs::remove_file(source).unwrap();
}

#[test]
fn original_decode_fails_instead_of_silently_reducing_resolution() {
    let _com = Com::new();
    let source = path("limit", "png");
    fixture(&source);
    let mut request = DecodeRequest::original(0);
    request.max_bytes = 4;
    assert!(ImageDocument::decode_request(&source, &|| false, None, &request).is_err());
    fs::remove_file(source).unwrap();
}

#[test]
fn externally_changed_sources_do_not_receive_old_edits() {
    let _com = Com::new();
    let source = path("changed", "png");
    fixture(&source);
    let document =
        ImageDocument::decode_request(&source, &|| false, None, &DecodeRequest::preview((2, 2)))
            .unwrap();
    fs::write(&source, b"changed").unwrap();
    assert!(
        document
            .render_view(DecodeRequest::original(0), &|| false, None)
            .is_err()
    );
    fs::remove_file(source).unwrap();
}

#[test]
fn tiff_pages_are_decoded_individually() {
    let _com = Com::new();
    let output = path("pages", "tif");
    let factory = ImageDocument::create_wic_factory().unwrap();
    let stream = unsafe { factory.CreateStream().unwrap() };
    let wide = wide_path(&output);
    unsafe {
        stream
            .InitializeFromFilename(PCWSTR(wide.as_ptr()), GENERIC_WRITE.0)
            .unwrap();
    }
    let encoder = unsafe {
        factory
            .CreateEncoder(&GUID_ContainerFormatTiff, std::ptr::null())
            .unwrap()
    };
    unsafe {
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .unwrap();
    }
    for (width, height, color) in [(2, 1, [0u8, 0, 255, 255]), (1, 3, [0, 255, 0, 255])] {
        let (mut frame, mut options) = (None, None);
        unsafe {
            encoder.CreateNewFrame(&mut frame, &mut options).unwrap();
        }
        let frame = frame.unwrap();
        let mut format = GUID_WICPixelFormat32bppPBGRA;
        unsafe {
            frame.Initialize(options.as_ref()).unwrap();
            frame.SetSize(width, height).unwrap();
            frame.SetPixelFormat(&mut format).unwrap();
        }
        let data = color.repeat(width as usize * height as usize);
        let bitmap = unsafe {
            factory
                .CreateBitmapFromMemory(
                    width,
                    height,
                    &GUID_WICPixelFormat32bppPBGRA,
                    width * 4,
                    &data,
                )
                .unwrap()
        };
        let bitmap: IWICBitmapSource = bitmap.cast().unwrap();
        let bitmap =
            convert_encoder_source(&factory, &bitmap, &GUID_WICPixelFormat32bppPBGRA, &format)
                .unwrap();
        unsafe {
            frame.WriteSource(&bitmap, std::ptr::null()).unwrap();
            frame.Commit().unwrap();
        }
    }
    unsafe {
        encoder.Commit().unwrap();
    }
    drop(encoder);
    drop(stream);
    let first =
        ImageDocument::decode_request(&output, &|| false, None, &DecodeRequest::original(0))
            .unwrap();
    let second = first
        .render_view(DecodeRequest::original(1), &|| false, None)
        .unwrap();
    assert_eq!(first.frame_count(), 2);
    assert_eq!(first.frames.len(), 1);
    assert_eq!(second.frames.len(), 1);
    assert_eq!(first.view_dimensions(), (2, 1));
    assert_eq!(second.view_dimensions(), (1, 3));
    assert_eq!(&second.current_frame().pixels[..4], &[0, 255, 0, 255]);
    fs::remove_file(output).unwrap();
}

#[test]
fn svg_region_rotation_and_crop_match_raster_geometry() {
    let _com = Com::new();
    let source = path("vector", "svg");
    fs::write(&source,r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6"><rect width="4" height="6" fill="red"/><rect x="4" width="4" height="6" fill="blue"/></svg>"#).unwrap();
    let original =
        ImageDocument::decode_request(&source, &|| false, None, &DecodeRequest::original(0))
            .unwrap();
    let mut request = DecodeRequest::original(0);
    request.operations = vec![EditOperation::RotateRight, EditOperation::Crop(1, 2, 5, 7)];
    let edited = ImageDocument::decode_request(&source, &|| false, None, &request).unwrap();
    let expected = render_raster_view(
        original.current_frame(),
        original.original_size,
        &request,
        &|| false,
    )
    .unwrap();
    assert_eq!(edited.current_frame().pixels, expected.pixels);
    fs::remove_file(source).unwrap();
}

#[test]
fn streamed_jpeg_export_flattens_transparency_on_white() {
    let _com = Com::new();
    let source = path("transparent", "png");
    let output = path("white", "jpg");
    let frame = Frame {
        width: 8,
        height: 6,
        pixels: PixelBuffer::zeroed(8 * 6 * 4).unwrap(),
        delay_ms: 100,
    };
    let mut document = raster_document(
        &source,
        frame,
        (8, 6),
        1,
        0,
        "PNG",
        &DecodeRequest::original(0),
    );
    document.source = None;
    let png = ExportFormat {
        container: GUID_ContainerFormatPng,
        name: "PNG".into(),
        extensions: vec!["png".into()],
    };
    document.save_as_format(&source, &png).unwrap();
    let preview =
        ImageDocument::decode_request(&source, &|| false, None, &DecodeRequest::preview((2, 2)))
            .unwrap();
    let jpeg = ExportFormat {
        container: GUID_ContainerFormatJpeg,
        name: "JPEG".into(),
        extensions: vec!["jpg".into()],
    };
    preview.export_copy_as(&output, &jpeg).unwrap();
    let exported =
        ImageDocument::decode_request(&output, &|| false, None, &DecodeRequest::original(0))
            .unwrap();
    assert_eq!(exported.view_dimensions(), (8, 6));
    assert!(exported.current_frame().pixels.iter().all(|&v| v >= 250));
    fs::remove_file(source).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
fn jpeg_xl_lossless_alpha_and_edited_regions_match_png() {
    let _com = Com::new();
    let png = path("jxl-reference", "png");
    let jxl = path("jxl-source", "jxl");
    fs::write(&png, include_bytes!("fixtures/lossless-rgba.png")).unwrap();
    fs::write(&jxl, include_bytes!("fixtures/lossless-rgba.jxl")).unwrap();
    let original =
        ImageDocument::decode_request(&png, &|| false, None, &DecodeRequest::original(0)).unwrap();
    let decoded =
        ImageDocument::decode_request(&jxl, &|| false, None, &DecodeRequest::original(0)).unwrap();
    assert_eq!(decoded.original_size, (17, 9));
    assert_eq!(
        decoded.current_frame().pixels,
        original.current_frame().pixels
    );
    let mut request = DecodeRequest::original(0);
    request.operations = vec![EditOperation::RotateRight, EditOperation::Crop(2, 3, 7, 14)];
    request.region = Some((1, 2, 4, 7));
    let region = ImageDocument::decode_request(&jxl, &|| false, None, &request).unwrap();
    let expected = render_raster_view(
        original.current_frame(),
        original.original_size,
        &request,
        &|| false,
    )
    .unwrap();
    assert_eq!(region.current_frame().pixels, expected.pixels);
    fs::remove_file(png).unwrap();
    fs::remove_file(jxl).unwrap();
}
