use std::{ffi::c_void, fs, io::Read, mem::size_of, path::Path};

use windows::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
        WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
    core::PCWSTR,
};

use crate::platform::text::wide;

const MAX_UPLOAD_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

struct InternetHandle(*mut c_void);

impl Drop for InternetHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub fn upload(path: &Path, api_key: &str) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_UPLOAD_BYTES {
        return Err("ImgBB admite imágenes de hasta 32 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_UPLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_UPLOAD_BYTES {
        return Err("ImgBB admite imágenes de hasta 32 MiB.".into());
    }
    let boundary = "----KGenView7MA4YWxkTrZu0gW";
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(['\r', '\n', '"'], "_");
    let prefix = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    let suffix = format!("\r\n--{boundary}--\r\n");
    let total = prefix.len() + bytes.len() + suffix.len();
    if total > u32::MAX as usize {
        return Err("La solicitud es demasiado grande.".into());
    }
    let mut body = Vec::with_capacity(total);
    body.extend_from_slice(prefix.as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(suffix.as_bytes());

    let agent = wide("KGenView/1.2");
    let host = wide("api.imgbb.com");
    let verb = wide("POST");
    let endpoint = wide(&format!("/1/upload?key={}", encode_query(api_key.trim())));
    let session = InternetHandle(unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err("No se pudo iniciar la conexión HTTPS.".into());
    }
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 30_000) }
        .map_err(|error| format!("No se pudieron configurar los tiempos de espera: {error}"))?;
    let connection =
        InternetHandle(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0) });
    if connection.0.is_null() {
        return Err("No se pudo conectar con ImgBB.".into());
    }
    let request = InternetHandle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(endpoint.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err("No se pudo preparar la subida.".into());
    }
    let headers: Vec<u16> = format!("Content-Type: multipart/form-data; boundary={boundary}\r\n")
        .encode_utf16()
        .collect();
    unsafe {
        WinHttpSendRequest(
            request.0,
            Some(&headers),
            Some(body.as_ptr().cast()),
            body.len() as u32,
            body.len() as u32,
            0,
        )
        .map_err(|error| format!("ImgBB rechazó la solicitud: {error}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|error| format!("ImgBB no devolvió una respuesta: {error}"))?;
    }
    let mut status_code = 0u32;
    let mut status_bytes = size_of::<u32>() as u32;
    let mut header_index = 0u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status_code as *mut u32).cast()),
            &mut status_bytes,
            &mut header_index,
        )
    }
    .map_err(|error| format!("No se pudo leer la respuesta de ImgBB: {error}"))?;
    let mut response = Vec::new();
    loop {
        let mut available = 0u32;
        unsafe { WinHttpQueryDataAvailable(request.0, &mut available) }
            .map_err(|error| error.to_string())?;
        if available == 0 {
            break;
        }
        if response.len().saturating_add(available as usize) > MAX_RESPONSE_BYTES {
            return Err("La respuesta de ImgBB supera el límite de 1 MiB.".into());
        }
        let offset = response.len();
        response.resize(offset + available as usize, 0);
        let mut read = 0u32;
        unsafe {
            WinHttpReadData(
                request.0,
                response[offset..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
        }
        .map_err(|error| error.to_string())?;
        response.truncate(offset + read as usize);
    }
    let json = String::from_utf8_lossy(&response);
    if !(200..300).contains(&status_code) {
        let details = json_field(&json, "message").or_else(|| json_field(&json, "error"));
        return Err(details.map_or_else(
            || format!("ImgBB respondió con HTTP {status_code}."),
            |message| format!("ImgBB respondió con HTTP {status_code}: {message}"),
        ));
    }
    direct_url(&json).ok_or_else(|| {
        if let Some(error) = json_field(&json, "message").or_else(|| json_field(&json, "error")) {
            format!("ImgBB: {error}")
        } else {
            "ImgBB no devolvió un enlace directo. Comprueba la clave API.".into()
        }
    })
}

fn direct_url(json: &str) -> Option<String> {
    let data = json.find("\"data\"").map(|index| &json[index..])?;
    json_field(data, "url").map(|value| value.replace("\\/", "/"))
}

fn json_field(json: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\"");
    let mut rest = &json[json.find(&needle)? + needle.len()..];
    rest = rest.trim_start();
    rest = rest.strip_prefix(':')?.trim_start();
    rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn encode_query(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_direct_image_url() {
        let json = r#"{"data":{"url_viewer":"https://ibb.co/x","url":"https:\/\/i.ibb.co\/x\/a.png"},"success":true}"#;
        assert_eq!(
            direct_url(json).as_deref(),
            Some("https://i.ibb.co/x/a.png")
        );
    }
}
