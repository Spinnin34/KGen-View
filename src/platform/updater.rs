use std::{
    ffi::{OsStr, OsString, c_void},
    fs::{self, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use windows::{
    Win32::{
        Foundation::{ERROR_SUCCESS, HWND, LPARAM, NTSTATUS, WPARAM},
        Networking::WinHttp::{
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
            WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
            WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle,
            WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
            WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
            WinHttpSetOption, WinHttpSetTimeouts,
        },
        Security::Cryptography::{
            BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_OBJECT_LENGTH,
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM,
            BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
            BCryptGetProperty, BCryptHashData, BCryptOpenAlgorithmProvider,
        },
        System::{
            Recovery::{REGISTER_APPLICATION_RESTART_FLAGS, RegisterApplicationRestart},
            Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
        },
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_APP},
        },
    },
    core::{PCWSTR, w},
};

use crate::{mailbox::Mailbox, platform::text::wide};

const REPOSITORY: &str = "Spinnin34/KGen-View";
const RELEASES_API: &str = "https://api.github.com/repos/Spinnin34/KGen-View/releases?per_page=20";
const MAX_RELEASES_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const INSTALLER_NAME: &str = "KGenView-Setup-x64.exe";
pub const WM_UPDATE_EVENT: u32 = WM_APP + 9;

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub channel: String,
    installer_url: String,
    installer_sha256: String,
    installer_size: u64,
}

pub enum UpdateEvent {
    CheckComplete(Result<Option<UpdateInfo>, String>),
    InstallStarted(Result<(), String>),
}

pub fn is_installed_copy() -> bool {
    let Some(registered_path) = installed_executable() else {
        return false;
    };
    let Ok(current_path) = std::env::current_exe() else {
        return false;
    };
    normalized_path(&registered_path) == normalized_path(&current_path)
}

pub fn start_check(
    hwnd: HWND,
    alive: Arc<AtomicBool>,
    events: Arc<Mailbox<UpdateEvent>>,
) -> Result<(), String> {
    if !is_installed_copy() {
        return Ok(());
    }
    let target = hwnd.0 as usize;
    thread::Builder::new()
        .name("kgen-update-check".into())
        .spawn(move || {
            publish_event(
                target,
                &alive,
                &events,
                UpdateEvent::CheckComplete(check_for_update()),
            )
        })
        .map(|_| ())
        .map_err(|error| format!("No se pudo iniciar la comprobación de actualizaciones: {error}"))
}

pub fn register_restart(path: Option<&Path>, archive_member: Option<(&Path, &str)>) {
    if !is_installed_copy() {
        return;
    }

    let mut arguments = Vec::new();
    if let Some((archive_path, member_name)) = archive_member {
        arguments.push(OsString::from("--open-archive-member"));
        arguments.push(archive_path.as_os_str().to_os_string());
        arguments.push(OsString::from(member_name));
    } else if let Some(path) = path {
        arguments.push(path.as_os_str().to_os_string());
    }
    if std::env::args_os().any(|argument| argument == "--mcp") {
        arguments.push(OsString::from("--mcp"));
    }

    let mut encoded = encode_restart_arguments(&arguments);
    if encoded.is_none()
        && let Some((archive_path, _)) = archive_member
    {
        let mut fallback = vec![archive_path.as_os_str().to_os_string()];
        if std::env::args_os().any(|argument| argument == "--mcp") {
            fallback.push(OsString::from("--mcp"));
        }
        encoded = encode_restart_arguments(&fallback);
    }
    let Some(encoded) = encoded else {
        return;
    };
    let command_line = if encoded.len() == 1 {
        PCWSTR::null()
    } else {
        PCWSTR(encoded.as_ptr())
    };
    unsafe {
        let _ = RegisterApplicationRestart(command_line, REGISTER_APPLICATION_RESTART_FLAGS(0));
    }
}

fn encode_restart_arguments(arguments: &[OsString]) -> Option<Vec<u16>> {
    let mut encoded = Vec::new();
    for argument in arguments {
        if !encoded.is_empty() {
            encoded.push(b' ' as u16);
        }
        quote_windows_argument(argument, &mut encoded);
        if encoded.len() > 1023 {
            return None;
        }
    }
    encoded.push(0);
    Some(encoded)
}

fn quote_windows_argument(argument: &OsStr, output: &mut Vec<u16>) {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;

    output.push(QUOTE);
    let mut backslashes = 0usize;
    for unit in argument.encode_wide() {
        if unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
            continue;
        }
        if unit == QUOTE {
            output.extend(std::iter::repeat_n(
                BACKSLASH,
                backslashes.saturating_mul(2) + 1,
            ));
        } else {
            output.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        }
        output.push(unit);
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n(
        BACKSLASH,
        backslashes.saturating_mul(2),
    ));
    output.push(QUOTE);
}

pub fn start_install(
    hwnd: HWND,
    alive: Arc<AtomicBool>,
    events: Arc<Mailbox<UpdateEvent>>,
    update: UpdateInfo,
) -> Result<(), String> {
    let target = hwnd.0 as usize;
    thread::Builder::new()
        .name("kgen-update-download".into())
        .spawn(move || {
            let result = download_verify_and_launch(&update);
            publish_event(target, &alive, &events, UpdateEvent::InstallStarted(result));
        })
        .map(|_| ())
        .map_err(|error| format!("No se pudo iniciar la descarga: {error}"))
}

fn publish_event(
    target: usize,
    alive: &AtomicBool,
    events: &Mailbox<UpdateEvent>,
    event: UpdateEvent,
) {
    if !alive.load(Ordering::Acquire) || !events.publish(event) {
        return;
    }
    unsafe {
        let _ = PostMessageW(
            Some(HWND(target as *mut c_void)),
            WM_UPDATE_EVENT,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let releases = get_https(RELEASES_API, MAX_RELEASES_BYTES)?;
    let releases: Value = serde_json::from_slice(&releases)
        .map_err(|error| format!("GitHub devolvió una lista de versiones no válida: {error}"))?;
    let releases = releases
        .as_array()
        .ok_or_else(|| "GitHub no devolvió una lista de versiones.".to_string())?;
    let published = releases
        .iter()
        .filter(|release| release.get("draft").and_then(Value::as_bool) == Some(false))
        .filter(|release| {
            release
                .get("published_at")
                .and_then(Value::as_str)
                .is_some()
        })
        .collect::<Vec<_>>();
    // All installed copies track beta builds by choice. Stable tags remain
    // available on GitHub, but cannot make clients move back to older code.
    let latest = published
        .iter()
        .copied()
        .filter_map(|release| beta_sequence(release).map(|sequence| (sequence, release)))
        .max_by_key(|(sequence, _)| *sequence)
        .map(|(_, release)| release)
        .or_else(|| {
            published
                .iter()
                .copied()
                .filter(|release| release.get("prerelease").and_then(Value::as_bool) == Some(false))
                .max_by_key(|release| {
                    release
                        .get("published_at")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
        });
    let Some(latest) = latest else {
        return Ok(None);
    };

    let tag = latest
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "La versión de GitHub no tiene una etiqueta válida.".to_string())?;
    if !valid_release_tag(tag) {
        return Err("La etiqueta de versión de GitHub no es válida.".into());
    }
    let manifest_url =
        format!("https://github.com/{REPOSITORY}/releases/download/{tag}/update.json");
    let manifest_asset_url = latest
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                (asset.get("name").and_then(Value::as_str) == Some("update.json"))
                    .then(|| asset.get("browser_download_url").and_then(Value::as_str))
                    .flatten()
            })
        })
        .ok_or_else(|| "La versión no incluye el manifiesto de actualización.".to_string())?;
    if manifest_asset_url != manifest_url {
        return Err("GitHub indicó una URL de manifiesto inesperada.".into());
    }
    let manifest_bytes = get_https(&manifest_url, MAX_MANIFEST_BYTES)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("El manifiesto de actualización no es válido: {error}"))?;

    if manifest.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("El manifiesto usa una versión no compatible.".into());
    }
    let version = required_string(&manifest, "version")?;
    let build_id = required_string(&manifest, "build_id")?;
    let channel = required_string(&manifest, "channel")?;
    let installer_url = required_string(&manifest, "installer_url")?;
    let installer_sha256 = required_string(&manifest, "installer_sha256")?.to_ascii_lowercase();
    let installer_size = manifest
        .get("installer_size")
        .and_then(Value::as_u64)
        .filter(|size| (1..=MAX_INSTALLER_BYTES).contains(size))
        .ok_or_else(|| "El tamaño del instalador no es válido.".to_string())?;
    let expected_channel = if latest.get("prerelease").and_then(Value::as_bool) == Some(true) {
        "beta"
    } else {
        "stable"
    };
    if channel != expected_channel {
        return Err("El canal indicado por GitHub no coincide con el manifiesto.".into());
    }
    if build_id.len() != 40 || !build_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("El identificador de compilación no es válido.".into());
    }
    if installer_sha256.len() != 64
        || !installer_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("La huella SHA-256 del instalador no es válida.".into());
    }
    let expected_installer_url =
        format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{INSTALLER_NAME}");
    if installer_url != expected_installer_url {
        return Err("GitHub indicó una URL de instalador inesperada.".into());
    }
    if build_id.eq_ignore_ascii_case(option_env!("KGEN_BUILD_ID").unwrap_or("local")) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version,
        channel: channel.to_string(),
        installer_url,
        installer_sha256,
        installer_size,
    }))
}

fn beta_sequence(release: &Value) -> Option<(u64, u64)> {
    if release.get("prerelease").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let tag = release.get("tag_name")?.as_str()?.strip_prefix("beta-")?;
    let mut parts = tag.split('-');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("Falta el campo {key} en el manifiesto."))
}

fn valid_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn download_verify_and_launch(update: &UpdateInfo) -> Result<(), String> {
    if !is_installed_copy() {
        return Err(
            "La actualización automática solo está disponible en la copia instalada.".into(),
        );
    }
    let destination = new_download_path()?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| format!("No se pudo preparar el archivo temporal: {error}"))?;
        let mut hasher = Sha256::new()?;
        let downloaded = stream_https(&update.installer_url, MAX_INSTALLER_BYTES, |chunk| {
            file.write_all(chunk)
                .map_err(|error| format!("No se pudo guardar el instalador: {error}"))?;
            hasher.update(chunk)
        })?;
        if downloaded != update.installer_size {
            return Err("La descarga está incompleta; el instalador no se ejecutará.".into());
        }
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("No se pudo finalizar el archivo descargado: {error}"))?;
        drop(file);
        let actual_hash = hasher.finish()?;
        if !actual_hash.eq_ignore_ascii_case(&update.installer_sha256) {
            return Err("La verificación SHA-256 falló; el instalador no se ejecutará.".into());
        }
        launch_installer(&destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&destination);
    }
    result
}

fn new_download_path() -> Result<PathBuf, String> {
    let temp = std::env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(temp.join(format!(
        "KGenView-update-{}-{timestamp}.exe",
        std::process::id()
    )))
}

fn launch_installer(path: &Path) -> Result<(), String> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let parameters = wide("/SILENT /SP- /CLOSEAPPLICATIONS /RESTARTAPPLICATIONS /NORESTART");
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err("Windows no pudo iniciar el instalador de KGen View.".into());
    }
    Ok(())
}

fn get_https(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    stream_https(url, limit, |chunk| {
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(body)
}

fn stream_https(
    url: &str,
    max_bytes: u64,
    mut on_chunk: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<u64, String> {
    let (host, path) = parse_github_url(url)?;
    let agent = wide(concat!("KGenView/", env!("CARGO_PKG_VERSION")));
    let host_wide = wide(&host);
    let verb = wide("GET");
    let path_wide = wide(&path);
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
        return Err("No se pudo iniciar la conexión HTTPS con GitHub.".into());
    }
    unsafe { WinHttpSetTimeouts(session.0, 5_000, 5_000, 10_000, 10_000) }
        .map_err(|error| format!("No se pudieron configurar los tiempos de espera: {error}"))?;
    let connection =
        InternetHandle(unsafe { WinHttpConnect(session.0, PCWSTR(host_wide.as_ptr()), 443, 0) });
    if connection.0.is_null() {
        return Err("No se pudo conectar con GitHub.".into());
    }
    let request = InternetHandle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err("No se pudo preparar la solicitud a GitHub.".into());
    }
    let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP.to_ne_bytes();
    unsafe {
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect_policy),
        )
        .map_err(|error| format!("No se pudo asegurar la redirección HTTPS: {error}"))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)
            .map_err(|error| format!("GitHub no aceptó la solicitud: {error}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|error| format!("GitHub no devolvió una respuesta: {error}"))?;
    }
    let mut status_code = 0u32;
    let mut status_bytes = std::mem::size_of::<u32>() as u32;
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
    .map_err(|error| format!("No se pudo leer el estado de GitHub: {error}"))?;
    if !(200..300).contains(&status_code) {
        return Err(format!("GitHub respondió con HTTP {status_code}."));
    }

    let mut downloaded = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let mut available = 0u32;
        unsafe { WinHttpQueryDataAvailable(request.0, &mut available) }
            .map_err(|error| format!("No se pudo leer la respuesta de GitHub: {error}"))?;
        if available == 0 {
            break;
        }
        let requested = available.min(buffer.len() as u32);
        let mut read = 0u32;
        unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), requested, &mut read) }
            .map_err(|error| format!("La descarga desde GitHub falló: {error}"))?;
        if read == 0 {
            break;
        }
        downloaded = downloaded
            .checked_add(read as u64)
            .filter(|size| *size <= max_bytes)
            .ok_or_else(|| "La respuesta de GitHub supera el tamaño permitido.".to_string())?;
        on_chunk(&buffer[..read as usize])?;
    }
    Ok(downloaded)
}

fn parse_github_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "Se rechazó una URL que no usa HTTPS.".to_string())?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| "La URL de GitHub no tiene una ruta válida.".to_string())?;
    if !matches!(host, "api.github.com" | "github.com") || path.is_empty() {
        return Err("Se rechazó una URL fuera de los servidores permitidos de GitHub.".into());
    }
    Ok((host.to_string(), format!("/{path}")))
}

fn installed_executable() -> Option<PathBuf> {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\KGenView.exe");
    let mut size = 0u32;
    let first = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if first != ERROR_SUCCESS || size < 2 {
        return None;
    }
    let mut value = vec![0u16; (size as usize).div_ceil(2)];
    let second = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if second != ERROR_SUCCESS {
        return None;
    }
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    Some(PathBuf::from(String::from_utf16_lossy(&value[..end])))
}

fn normalized_path(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

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

struct Sha256 {
    algorithm: BCRYPT_ALG_HANDLE,
    hash: BCRYPT_HASH_HANDLE,
    _object: Vec<u8>,
}

impl Sha256 {
    fn new() -> Result<Self, String> {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        let status = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_SHA256_ALGORITHM,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        ensure_nt_success(status, "iniciar SHA-256")?;

        let mut object_length_bytes = [0u8; 4];
        let mut result_length = 0u32;
        let status = unsafe {
            BCryptGetProperty(
                algorithm.into(),
                BCRYPT_OBJECT_LENGTH,
                Some(&mut object_length_bytes),
                &mut result_length,
                0,
            )
        };
        if let Err(error) = ensure_nt_success(status, "preparar SHA-256") {
            unsafe {
                let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
            }
            return Err(error);
        }
        let object_length = u32::from_ne_bytes(object_length_bytes) as usize;
        if object_length == 0 || object_length > 1024 * 1024 {
            unsafe {
                let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
            }
            return Err("Windows devolvió un tamaño SHA-256 no válido.".into());
        }
        let mut object = vec![0u8; object_length];
        let mut hash = BCRYPT_HASH_HANDLE::default();
        let status = unsafe { BCryptCreateHash(algorithm, &mut hash, Some(&mut object), None, 0) };
        if let Err(error) = ensure_nt_success(status, "crear hash SHA-256") {
            unsafe {
                let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
            }
            return Err(error);
        }
        Ok(Self {
            algorithm,
            hash,
            _object: object,
        })
    }

    fn update(&mut self, bytes: &[u8]) -> Result<(), String> {
        let status = unsafe { BCryptHashData(self.hash, bytes, 0) };
        ensure_nt_success(status, "calcular SHA-256")
    }

    fn finish(self) -> Result<String, String> {
        let mut digest = [0u8; 32];
        let status = unsafe { BCryptFinishHash(self.hash, &mut digest, 0) };
        ensure_nt_success(status, "finalizar SHA-256")?;
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        unsafe {
            if !self.hash.0.is_null() {
                let _ = BCryptDestroyHash(self.hash);
            }
            if !self.algorithm.0.is_null() {
                let _ = BCryptCloseAlgorithmProvider(self.algorithm, 0);
            }
        }
    }
}

fn ensure_nt_success(status: NTSTATUS, operation: &str) -> Result<(), String> {
    if status.0 < 0 {
        Err(format!(
            "Windows no pudo {operation} (NTSTATUS 0x{:08X}).",
            status.0 as u32
        ))
    } else {
        Ok(())
    }
}
