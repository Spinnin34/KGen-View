//! Stream native WIC sources to encoders without a full-resolution CPU copy.
use super::*;
use windows::Win32::Graphics::Imaging::GUID_WICPixelFormat24bppBGR;

pub(super) fn is_wic_source(path: &Path) -> bool {
    !path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "gif" | "svg" | "jxl"
            )
        })
}

impl ImageDocument {
    pub(super) fn encode_native_wic(
        &self,
        factory: &IWICImagingFactory,
        path: &Path,
        container: windows::core::GUID,
        extension: &str,
        metadata_dirty: bool,
    ) -> ImageResult<()> {
        let wide = wide_path(&self.path);
        let decoder = unsafe {
            factory.CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?
        };
        let frame = unsafe { decoder.GetFrame(self.frame_index as u32)? };
        let reader = unsafe { frame.GetMetadataQueryReader().ok() };
        let orientation = reader
            .as_ref()
            .and_then(|r| read_metadata_u32(r, w!("/app1/ifd/{ushort=274}")))
            .or_else(|| {
                reader
                    .as_ref()
                    .and_then(|r| read_metadata_u32(r, w!("/ifd/{ushort=274}")))
            })
            .unwrap_or(1);
        let mut request = DecodeRequest::original(self.frame_index);
        request.operations = self.edits.operations.clone();
        let source = prepare_wic_view(
            factory,
            color_managed_source(factory, &frame)?,
            orientation,
            self.original_size,
            &request,
        )?;
        let (width, height) = self.view_dimensions();
        let stream = unsafe { factory.CreateStream()? };
        let wide = wide_path(path);
        unsafe {
            stream.InitializeFromFilename(PCWSTR(wide.as_ptr()), GENERIC_WRITE.0)?;
        }
        let encoder = unsafe { factory.CreateEncoder(&container, std::ptr::null())? };
        unsafe {
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        }
        let (mut encoded, mut options) = (None, None);
        unsafe {
            encoder.CreateNewFrame(&mut encoded, &mut options)?;
        }
        let encoded =
            encoded.ok_or_else(|| ImageError("No se pudo crear el codificador.".into()))?;
        let mut format = if container == GUID_ContainerFormatJpeg {
            GUID_WICPixelFormat24bppBGR
        } else {
            GUID_WICPixelFormat32bppPBGRA
        };
        let mut dpi = valid_resolution(self.resolution_dpi);
        if self
            .edits
            .operations
            .iter()
            .filter(|op| matches!(op, EditOperation::RotateLeft | EditOperation::RotateRight))
            .count()
            % 2
            != 0
        {
            dpi = (dpi.1, dpi.0);
        }
        unsafe {
            encoded.Initialize(options.as_ref())?;
            encoded.SetSize(width, height)?;
            encoded.SetResolution(dpi.0, dpi.1)?;
            encoded.SetPixelFormat(&mut format)?;
        }
        if container == GUID_ContainerFormatJpeg && format == GUID_WICPixelFormat24bppBGR {
            let rows = (crate::resources::MIB / (width as usize * 4))
                .max(1)
                .min(height as usize);
            let mut bgra = PixelBuffer::zeroed(width as usize * rows * 4)?;
            let mut bgr = PixelBuffer::zeroed(width as usize * rows * 3)?;
            for top in (0..height as usize).step_by(rows) {
                let count = rows.min(height as usize - top);
                let rect = WICRect {
                    X: 0,
                    Y: top as i32,
                    Width: width as i32,
                    Height: count as i32,
                };
                let input = &mut bgra.try_make_mut()?[..count * width as usize * 4];
                unsafe {
                    source.CopyPixels(&rect, width * 4, input)?;
                }
                let output = &mut bgr.try_make_mut()?[..count * width as usize * 3];
                for (pixel, target) in input
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(output.as_chunks_mut::<3>().0.iter_mut())
                {
                    let white = 255u16 - u16::from(pixel[3]);
                    for channel in 0..3 {
                        target[channel] = (u16::from(pixel[channel]) + white).min(255) as u8;
                    }
                }
                unsafe {
                    encoded.WritePixels(count as u32, width * 3, output)?;
                }
            }
        } else {
            let source =
                convert_encoder_source(factory, &source, &GUID_WICPixelFormat32bppPBGRA, &format)?;
            unsafe {
                encoded.WriteSource(&source, std::ptr::null())?;
            }
        }
        write_editable_metadata(
            &unsafe { encoded.GetMetadataQueryWriter() },
            extension,
            &self.metadata,
            metadata_dirty,
        )?;
        unsafe {
            encoded.Commit()?;
            encoder.Commit()?;
        }
        Ok(())
    }
}
