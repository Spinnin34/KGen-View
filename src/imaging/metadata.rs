use super::*;

pub(super) fn write_editable_metadata(
    writer: &windows::core::Result<IWICMetadataQueryWriter>,
    extension: &str,
    metadata: &[(String, String)],
    metadata_dirty: bool,
) -> ImageResult<()> {
    let author = metadata
        .iter()
        .find(|(label, _)| label == "Autor")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    let description = metadata
        .iter()
        .find(|(label, _)| label == "Descripción")
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    if author.is_empty() && description.is_empty() {
        return Ok(());
    }

    let queries = match extension {
        "jpg" | "jpeg" | "jpe" => [
            ("Autor", w!("/app1/ifd/{ushort=315}")),
            ("Descripción", w!("/app1/ifd/{ushort=270}")),
        ],
        "tif" | "tiff" => [
            ("Autor", w!("/ifd/{ushort=315}")),
            ("Descripción", w!("/ifd/{ushort=270}")),
        ],
        "png" => [
            ("Autor", w!("/tEXt/{str=Author}")),
            ("Descripción", w!("/tEXt/{str=Description}")),
        ],
        _ => {
            if !metadata_dirty {
                return Ok(());
            }
            return Err(ImageError(
                "El formato elegido no admite guardar autor y descripción. Usa PNG, JPEG o TIFF."
                    .into(),
            ));
        }
    };
    let writer = writer
        .as_ref()
        .map_err(|error| ImageError(error.to_string()))?;
    for (label, query) in queries {
        let value = if label == "Autor" {
            author
        } else {
            description
        };
        if !value.is_empty() {
            write_metadata_string(writer, query, value)?;
        }
    }
    Ok(())
}

fn write_metadata_string(
    writer: &IWICMetadataQueryWriter,
    query: PCWSTR,
    value: &str,
) -> ImageResult<()> {
    let mut wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut variant = PROPVARIANT::default();
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = windows::core::PWSTR(wide.as_mut_ptr());
        writer.SetMetadataByName(query, &variant)?;
    }
    Ok(())
}

pub(super) fn read_metadata_u32(reader: &IWICMetadataQueryReader, query: PCWSTR) -> Option<u32> {
    let mut value = PROPVARIANT::default();
    unsafe { reader.GetMetadataByName(query, &mut value) }.ok()?;
    let result = unsafe {
        let inner = &*value.Anonymous.Anonymous;
        if inner.vt == VT_UI2 {
            Some(inner.Anonymous.uiVal as u32)
        } else if inner.vt == VT_UI4 {
            Some(inner.Anonymous.ulVal)
        } else if inner.vt == VT_I2 {
            Some(inner.Anonymous.iVal.max(0) as u32)
        } else {
            None
        }
    };
    unsafe {
        let _ = PropVariantClear(&mut value);
    }
    result
}

pub(super) fn read_metadata_string(
    reader: &IWICMetadataQueryReader,
    query: PCWSTR,
) -> Option<String> {
    let mut value = PROPVARIANT::default();
    unsafe { reader.GetMetadataByName(query, &mut value) }.ok()?;
    let result = unsafe {
        let inner = &*value.Anonymous.Anonymous;
        if inner.vt == VT_LPWSTR {
            inner.Anonymous.pwszVal.to_string().ok()
        } else if inner.vt == VT_LPSTR {
            inner.Anonymous.pszVal.to_string().ok()
        } else if inner.vt == VT_BSTR {
            Some(inner.Anonymous.bstrVal.to_string())
        } else if inner.vt == VT_UI2 {
            Some(inner.Anonymous.uiVal.to_string())
        } else if inner.vt == VT_UI4 {
            Some(inner.Anonymous.ulVal.to_string())
        } else {
            None
        }
    };
    unsafe {
        let _ = PropVariantClear(&mut value);
    }
    result.filter(|text| !text.trim().is_empty())
}
