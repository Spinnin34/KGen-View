use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use crate::{
    catalog::{is_supported, natural_cmp},
    i18n::Language,
    imaging::{ImageError, ImageResult},
};

const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_ARCHIVE_IMAGES: usize = 20_000;
const MAX_ARCHIVE_MEMBER_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PREFETCH_FILE_BYTES: u64 = 16 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 128 * 1024;
static TEMP_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct ArchiveMember {
    path: PathBuf,
    name: String,
    index: usize,
    size: u64,
    extraction_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
pub(super) struct ArchiveMemberRef {
    source_path: PathBuf,
    path: PathBuf,
    index: usize,
    size: u64,
    extraction_lock: Arc<Mutex<()>>,
    storage: Arc<ArchiveStorage>,
    source_stamp: (u64, Option<SystemTime>),
}

pub(super) struct ArchiveSession {
    source_path: PathBuf,
    storage: Arc<ArchiveStorage>,
    source_stamp: (u64, Option<SystemTime>),
    members: Vec<ArchiveMember>,
    positions: HashMap<PathBuf, usize>,
}

#[derive(Debug)]
struct ArchiveStorage {
    root: PathBuf,
    leases: Mutex<HashMap<PathBuf, Weak<ExtractedFile>>>,
}

#[derive(Debug)]
pub(super) struct ExtractedFile {
    path: PathBuf,
    storage: Arc<ArchiveStorage>,
}

struct ArchiveEntry {
    index: usize,
    name: String,
    extension: String,
    size: u64,
}

pub(super) fn is_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "zip" | "cbz"))
}

impl ArchiveSession {
    pub fn open(
        source_path: &Path,
        should_cancel: &impl Fn() -> bool,
        language: Language,
    ) -> ImageResult<Self> {
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let file = File::open(source_path).map_err(ImageError::from)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| archive_error(language, &error.to_string()))?;
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(localized_error(
                language,
                "El ZIP contiene demasiados archivos para abrirlo.",
                "The ZIP archive contains too many files to open.",
            ));
        }

        let mut entries = Vec::new();
        for index in 0..archive.len() {
            if should_cancel() {
                return Err(ImageError("Carga cancelada.".into()));
            }
            let Ok(file) = archive.by_index(index) else {
                continue;
            };
            if file.is_dir() {
                continue;
            }
            let Some(member_path) = file.enclosed_name() else {
                continue;
            };
            if !is_supported(&member_path) {
                continue;
            }
            let Some(extension) = member_path.extension().and_then(|value| value.to_str()) else {
                continue;
            };
            entries.push(ArchiveEntry {
                index,
                name: member_path.to_string_lossy().replace('\\', "/"),
                extension: extension.to_ascii_lowercase(),
                size: file.size(),
            });
        }
        entries.sort_by(|left, right| {
            natural_cmp(&left.name, &right.name).then_with(|| left.index.cmp(&right.index))
        });
        if entries.len() > MAX_ARCHIVE_IMAGES {
            return Err(localized_error(
                language,
                "El ZIP contiene demasiadas imágenes para abrirlo de una vez.",
                "The ZIP archive contains too many images to open at once.",
            ));
        }
        if entries.is_empty() {
            return Err(no_images_error(language));
        }

        let storage = Arc::new(ArchiveStorage {
            root: create_temp_directory().map_err(ImageError::from)?,
            leases: Mutex::new(HashMap::new()),
        });
        let members = entries
            .into_iter()
            .enumerate()
            .map(|(ordinal, entry)| ArchiveMember {
                path: storage
                    .root
                    .join(format!("image-{ordinal:05}.{}", entry.extension)),
                name: entry.name,
                index: entry.index,
                size: entry.size,
                extraction_lock: Arc::new(Mutex::new(())),
            })
            .collect();

        let members: Vec<ArchiveMember> = members;
        let positions = members
            .iter()
            .enumerate()
            .map(|(i, m)| (m.path.clone(), i))
            .collect();
        let metadata = fs::metadata(source_path)?;
        Ok(Self {
            source_path: source_path.to_path_buf(),
            storage,
            source_stamp: (metadata.len(), metadata.modified().ok()),
            members,
            positions,
        })
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn image_paths(&self) -> Vec<PathBuf> {
        self.members
            .iter()
            .map(|member| member.path.clone())
            .collect()
    }

    pub fn first_member(&self) -> Option<ArchiveMemberRef> {
        self.members
            .first()
            .map(|member| self.make_member_ref(member))
    }

    pub fn member_ref_by_name(&self, name: &str) -> Option<ArchiveMemberRef> {
        let normalized_name = name.replace('\\', "/");
        self.members
            .iter()
            .find(|member| member.name.eq_ignore_ascii_case(&normalized_name))
            .map(|member| self.make_member_ref(member))
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.positions.contains_key(path)
    }
    pub fn member_name(&self, path: &Path) -> Option<&str> {
        self.positions
            .get(path)
            .map(|&i| self.members[i].name.as_str())
    }
    pub fn member_ref(&self, path: &Path) -> Option<ArchiveMemberRef> {
        self.positions
            .get(path)
            .map(|&i| self.make_member_ref(&self.members[i]))
    }

    fn make_member_ref(&self, member: &ArchiveMember) -> ArchiveMemberRef {
        ArchiveMemberRef {
            source_path: self.source_path.clone(),
            path: member.path.clone(),
            index: member.index,
            size: member.size,
            extraction_lock: member.extraction_lock.clone(),
            storage: self.storage.clone(),
            source_stamp: self.source_stamp,
        }
    }
}

impl ArchiveMemberRef {
    pub fn lease(&self) -> Arc<ExtractedFile> {
        let mut leases = self
            .storage
            .leases
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(lease) = leases.get(&self.path).and_then(Weak::upgrade) {
            return lease;
        }
        let lease = Arc::new(ExtractedFile {
            path: self.path.clone(),
            storage: self.storage.clone(),
        });
        leases.insert(self.path.clone(), Arc::downgrade(&lease));
        lease
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn can_prefetch(&self) -> bool {
        self.size <= MAX_PREFETCH_FILE_BYTES
    }

    pub fn extract(
        &self,
        should_cancel: &impl Fn() -> bool,
        language: Language,
    ) -> ImageResult<()> {
        let _guard = self
            .extraction_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if should_cancel() {
            return Err(ImageError("Carga cancelada.".into()));
        }
        let metadata = fs::metadata(&self.source_path)?;
        if (metadata.len(), metadata.modified().ok()) != self.source_stamp {
            return Err(localized_error(
                language,
                "El ZIP cambió. Vuelve a abrirlo.",
                "The ZIP archive changed. Open it again.",
            ));
        }
        if let Ok(metadata) = fs::metadata(&self.path)
            && metadata.len() == self.size
        {
            return Ok(());
        }
        if self.size > MAX_ARCHIVE_MEMBER_BYTES {
            return Err(localized_error(
                language,
                "Esta imagen del ZIP supera el límite de extracción de 512 MiB.",
                "This image in the ZIP exceeds the 512 MiB extraction limit.",
            ));
        }
        let _ = fs::remove_file(&self.path);
        let partial_path = self.path.with_extension("partial");
        let _ = fs::remove_file(&partial_path);

        let extraction = self.extract_to_partial(&partial_path, should_cancel, language);
        if let Err(error) = extraction {
            let _ = fs::remove_file(&partial_path);
            return Err(error);
        }
        fs::rename(&partial_path, &self.path).map_err(ImageError::from)
    }

    fn extract_to_partial(
        &self,
        partial_path: &Path,
        should_cancel: &impl Fn() -> bool,
        language: Language,
    ) -> ImageResult<()> {
        ZIP_READER.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_none_or(|cache| {
                cache.path != self.source_path || cache.stamp != self.source_stamp
            }) {
                let file = File::open(&self.source_path)?;
                let archive = zip::ZipArchive::new(file)
                    .map_err(|e| archive_error(language, &e.to_string()))?;
                *slot = Some(ZipReader {
                    path: self.source_path.clone(),
                    stamp: self.source_stamp,
                    archive,
                });
            }
            let archive = &mut slot.as_mut().expect("ZIP reader").archive;
            let mut zipped_file = archive
                .by_index(self.index)
                .map_err(|error| archive_error(language, &error.to_string()))?;
            if zipped_file.size() != self.size {
                return Err(localized_error(
                    language,
                    "El archivo ZIP cambió mientras se estaba leyendo.",
                    "The ZIP archive changed while it was being read.",
                ));
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(partial_path)
                .map_err(ImageError::from)?;
            let mut image_bytes = 0u64;
            let mut buffer = [0u8; COPY_BUFFER_BYTES];
            loop {
                if should_cancel() {
                    return Err(ImageError("Carga cancelada.".into()));
                }
                let count = zipped_file
                    .read(&mut buffer)
                    .map_err(|error| archive_error(language, &error.to_string()))?;
                if count == 0 {
                    break;
                }
                image_bytes = image_bytes.saturating_add(count as u64);
                if image_bytes > MAX_ARCHIVE_MEMBER_BYTES {
                    return Err(localized_error(
                        language,
                        "Esta imagen del ZIP supera el límite de extracción de 512 MiB.",
                        "This image in the ZIP exceeds the 512 MiB extraction limit.",
                    ));
                }
                output
                    .write_all(&buffer[..count])
                    .map_err(ImageError::from)?;
            }
            if image_bytes != self.size {
                return Err(localized_error(
                    language,
                    "La imagen dentro del ZIP está incompleta o dañada.",
                    "The image inside the ZIP is incomplete or damaged.",
                ));
            }
            output.flush().map_err(ImageError::from)
        })
    }
}

struct ZipReader {
    path: PathBuf,
    stamp: (u64, Option<SystemTime>),
    archive: zip::ZipArchive<File>,
}
thread_local! { static ZIP_READER:RefCell<Option<ZipReader>>=const { RefCell::new(None) }; }

#[derive(Default)]
struct CleanupQueue {
    directories: HashSet<PathBuf>,
    files: VecDeque<(PathBuf, Arc<ArchiveStorage>)>,
    active: bool,
}
fn cleanup_queue() -> &'static Arc<(Mutex<CleanupQueue>, Condvar)> {
    static QUEUE: OnceLock<Arc<(Mutex<CleanupQueue>, Condvar)>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let queue = Arc::new((Mutex::new(CleanupQueue::default()), Condvar::new()));
        let worker = queue.clone();
        std::thread::Builder::new()
            .name("kgen-temp-cleanup".into())
            .spawn(move || {
                loop {
                    let (directory, file) = {
                        let mut state = worker.0.lock().unwrap_or_else(|e| e.into_inner());
                        while state.directories.is_empty() && state.files.is_empty() {
                            state = worker.1.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        state.active = true;
                        if let Some(path) = state.directories.iter().next().cloned() {
                            state.directories.remove(&path);
                            (Some(path), None)
                        } else {
                            (None, state.files.pop_front())
                        }
                    };
                    if let Some(path) = directory {
                        let _ = fs::remove_dir_all(path);
                    }
                    if let Some((path, storage)) = file {
                        {
                            let mut leases =
                                storage.leases.lock().unwrap_or_else(|e| e.into_inner());
                            if leases
                                .get(&path)
                                .is_none_or(|lease| lease.strong_count() == 0)
                            {
                                leases.remove(&path);
                                let _ = fs::remove_file(&path);
                                let _ = fs::remove_file(path.with_extension("partial"));
                            }
                        }
                        drop(storage);
                    }
                    let mut state = worker.0.lock().unwrap_or_else(|e| e.into_inner());
                    state.active = false;
                    worker.1.notify_all();
                }
            })
            .expect("start temporary cleanup worker");
        queue
    })
}
impl Drop for ExtractedFile {
    fn drop(&mut self) {
        let queue = cleanup_queue();
        let mut state = queue.0.lock().unwrap_or_else(|e| e.into_inner());
        // Overflowed individual cleanup is covered by eventual directory cleanup.
        if state.files.len() < 256 {
            state
                .files
                .push_back((self.path.clone(), self.storage.clone()));
        }
        queue.1.notify_one();
    }
}
impl Drop for ArchiveStorage {
    fn drop(&mut self) {
        let queue = cleanup_queue();
        queue
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .directories
            .insert(self.root.clone());
        queue.1.notify_one();
    }
}

fn create_temp_directory() -> std::io::Result<PathBuf> {
    let root = std::env::temp_dir();
    for _ in 0..32 {
        let id = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = root.join(format!("KGenView-{}-archive-{id}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "No se pudo crear una carpeta temporal para el ZIP.",
    ))
}

fn no_images_error(language: Language) -> ImageError {
    localized_error(
        language,
        "No se encontraron imágenes compatibles dentro del ZIP.",
        "No supported images were found inside the ZIP archive.",
    )
}

fn archive_error(language: Language, detail: &str) -> ImageError {
    if language == Language::Spanish {
        ImageError(format!("No se pudo leer el archivo ZIP: {detail}"))
    } else {
        ImageError(format!("Could not read the ZIP archive: {detail}"))
    }
}

fn localized_error(language: Language, spanish: &str, english: &str) -> ImageError {
    ImageError(if language == Language::Spanish {
        spanish.into()
    } else {
        english.into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn extracted_sources_outlive_the_archive_window_and_are_cleaned_after_release() {
        let path =
            std::env::temp_dir().join(format!("kgen-archive-lease-{}.zip", std::process::id()));
        let data = include_bytes!("../../../resources/kgen-view-logo.png");
        {
            let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("image2.png", options).unwrap();
            writer.write_all(data).unwrap();
            writer.start_file("image10.png", options).unwrap();
            writer.write_all(data).unwrap();
            writer.start_file("../outside.png", options).unwrap();
            writer.write_all(data).unwrap();
            writer.finish().unwrap();
        }
        let archive = ArchiveSession::open(&path, &|| false, Language::Spanish).unwrap();
        assert_eq!(archive.image_paths().len(), 2);
        let member = archive.first_member().unwrap();
        let lease = member.lease();
        member.extract(&|| false, Language::Spanish).unwrap();
        let extracted = member.path().to_path_buf();
        let directory = extracted.parent().unwrap().to_path_buf();
        assert_eq!(fs::read(&extracted).unwrap(), data);
        drop(member);
        drop(archive);
        assert!(extracted.exists());
        drop(lease);
        let started = Instant::now();
        while directory.exists() && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !directory.exists(),
            "temporary directory retained after the last lease"
        );
        ZIP_READER.with(|slot| slot.borrow_mut().take());
        fs::remove_file(path).unwrap();
    }
}
