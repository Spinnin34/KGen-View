use std::{
    cmp::Ordering,
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const EXTENSIONS: &[&str] = &[
    "bmp", "dib", "jpg", "jpeg", "jpe", "png", "gif", "tif", "tiff", "ico", "webp", "heic", "heif",
    "jxl", "wdp", "jxr", "hdp", "dds", "svg",
];

#[derive(Default, Debug, Clone)]
pub struct FolderCatalog {
    pub files: Arc<Vec<PathBuf>>,
    positions: Arc<HashMap<String, usize>>,
    pub index: Option<usize>,
    kind: MediaKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    Image,
    Video,
}

impl MediaKind {
    pub fn of(path: &Path) -> Self {
        if is_video(path) {
            Self::Video
        } else {
            Self::Image
        }
    }
}

impl FolderCatalog {
    pub fn scan_cancellable(
        current: &Path,
        explorer_window: usize,
        should_cancel: &impl Fn() -> bool,
    ) -> Self {
        let Some(folder) = current.parent() else {
            return Self::default();
        };
        let Ok(read_dir) = fs::read_dir(folder) else {
            return Self::default();
        };
        let kind = MediaKind::of(current);
        let supported = match kind {
            MediaKind::Image => is_supported,
            MediaKind::Video => is_video,
        };
        let mut entries = Vec::new();
        for entry in read_dir {
            if should_cancel() {
                return Self::default();
            }
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !supported(&path) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_file() {
                entries.push(path);
            }
        }
        if kind == MediaKind::Image
            && !should_cancel()
            && let Some(names) =
                crate::platform::explorer::folder_file_names(folder, explorer_window, should_cancel)
            && let Some(files) = files_in_view_order(&entries, current, names)
        {
            return Self::from_files(files, Some(current), kind);
        }
        if should_cancel() {
            return Self::default();
        }
        let mut entries: Vec<_> = entries
            .into_iter()
            .map(|path| (display_name(&path), path))
            .collect();
        entries.sort_unstable_by(|(a_name, a_path), (b_name, b_path)| {
            natural_cmp(a_name, b_name).then_with(|| a_path.cmp(b_path))
        });
        if should_cancel() {
            return Self::default();
        }
        let files = entries.into_iter().map(|(_, path)| path).collect();
        Self::from_files(files, Some(current), kind)
    }

    pub fn from_files(files: Vec<PathBuf>, current: Option<&Path>, kind: MediaKind) -> Self {
        let positions: HashMap<_, _> = files
            .iter()
            .enumerate()
            .map(|(index, path)| (path_key(path), index))
            .collect();
        let index = current.and_then(|path| positions.get(&path_key(path)).copied());
        Self {
            files: Arc::new(files),
            positions: Arc::new(positions),
            index,
            kind,
        }
    }

    pub fn matches(&self, path: &Path, kind: MediaKind) -> bool {
        self.kind == kind
            && self
                .files
                .first()
                .is_some_and(|entry| entry.parent() == path.parent())
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.positions.contains_key(&path_key(path))
    }

    pub fn remove(&mut self, path: &Path) {
        let files = self
            .files
            .iter()
            .filter(|item| !same_path(item, path))
            .cloned()
            .collect();
        *self = Self::from_files(files, None, self.kind);
    }

    pub fn update_index(&mut self, current: &Path) {
        self.index = self.positions.get(&path_key(current)).copied();
    }

    pub fn adjacent_wrapped(&self, offset: isize) -> Option<PathBuf> {
        self.adjacent_at(self.index?, offset)
    }

    pub fn adjacent_from(&self, path: &Path, offset: isize) -> Option<PathBuf> {
        self.adjacent_at(*self.positions.get(&path_key(path))?, offset)
    }

    fn adjacent_at(&self, current: usize, offset: isize) -> Option<PathBuf> {
        if self.files.len() < 2 {
            return None;
        }
        let index = (current as isize + offset).rem_euclid(self.files.len() as isize) as usize;
        self.files.get(index).cloned()
    }

    pub fn first(&self) -> Option<PathBuf> {
        self.files.first().cloned()
    }
    pub fn last(&self) -> Option<PathBuf> {
        self.files.last().cloned()
    }
}

fn files_in_view_order(
    entries: &[PathBuf],
    current: &Path,
    names: Vec<OsString>,
) -> Option<Vec<PathBuf>> {
    let mut by_name: HashMap<_, _> = entries
        .iter()
        .filter_map(|path| Some((path.file_name()?, path)))
        .collect();
    let files: Vec<_> = names
        .iter()
        .filter_map(|name| by_name.remove(name.as_os_str()))
        .cloned()
        .collect();
    // A filtered or out-of-date view may omit the open file. Fall back to
    // the folder scan so navigation can still start from that file.
    files
        .iter()
        .any(|path| same_path(path, current))
        .then_some(files)
}

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            EXTENSIONS
                .iter()
                .any(|extension| value.eq_ignore_ascii_case(extension))
        })
        .unwrap_or(false)
}

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "3g2", "3gp", "3gpp", "asf", "avi", "divx", "dv", "f4v", "flv", "h264", "h265", "hevc", "m1v",
    "m2p", "m2t", "m2ts", "m2v", "m4v", "mkv", "mov", "mp4", "mpe", "mpeg", "mpg", "mpv", "mts",
    "mxf", "nsv", "nut", "ogm", "ogv", "rm", "rmvb", "ts", "vob", "webm", "wmv", "wtv", "y4m",
];

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|extension| value.eq_ignore_ascii_case(extension))
        })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = 0;
    let mut bi = 0;
    let aa = a.as_bytes();
    let bb = b.as_bytes();
    while ai < aa.len() && bi < bb.len() {
        if aa[ai].is_ascii_digit() && bb[bi].is_ascii_digit() {
            let mut ae = ai;
            let mut be = bi;
            while ae < aa.len() && aa[ae].is_ascii_digit() {
                ae += 1;
            }
            while be < bb.len() && bb[be].is_ascii_digit() {
                be += 1;
            }
            let an = &a[ai..ae].trim_start_matches('0');
            let bn = &b[bi..be].trim_start_matches('0');
            let ord = an.len().cmp(&bn.len()).then_with(|| an.cmp(bn));
            if ord != Ordering::Equal {
                return ord;
            }
            ai = ae;
            bi = be;
        } else {
            let ac = a[ai..].chars().next().unwrap();
            let bc = b[bi..].chars().next().unwrap();
            let ord = if ac.is_ascii() && bc.is_ascii() {
                ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase())
            } else {
                ac.to_lowercase().cmp(bc.to_lowercase())
            };
            if ord != Ordering::Equal {
                return ord;
            }
            ai += ac.len_utf8();
            bi += bc.len_utf8();
        }
    }
    (aa.len() - ai)
        .cmp(&(bb.len() - bi))
        .then_with(|| aa.len().cmp(&bb.len()))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_is_human_friendly() {
        let mut names = vec!["image10.png", "image2.png", "image01.png", "Image1.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            vec!["Image1.png", "image01.png", "image2.png", "image10.png"]
        );
    }

    #[test]
    fn natural_order_compares_prefixes_ignoring_leading_zeroes() {
        assert_eq!(natural_cmp("a000", "a0b"), Ordering::Less);
    }

    #[test]
    fn explorer_order_filters_the_scan_and_keeps_the_open_file_as_anchor() {
        let files: Vec<_> = ["image1.png", "image2.png", "image10.png", "hidden.png"]
            .into_iter()
            .map(|name| Path::new("photos").join(name))
            .collect();
        let names = [
            "image10.png",
            "video.mp4",
            "image2.png",
            "image2.png",
            "image1.png",
        ]
        .map(OsString::from);
        let ordered = files_in_view_order(&files, &files[1], names.to_vec()).unwrap();
        let catalog = FolderCatalog::from_files(ordered, Some(&files[1]), MediaKind::Image);
        assert_eq!(catalog.adjacent_wrapped(1), Some(files[0].clone()));
        assert_eq!(catalog.adjacent_wrapped(-1), Some(files[2].clone()));
        assert_eq!(catalog.adjacent_from(&files[0], 1), Some(files[2].clone()));
        assert_eq!(catalog.index, Some(1));
        assert_eq!(catalog.files.len(), 3);
        assert!(files_in_view_order(&files, &files[3], names.to_vec()).is_none());
    }

    #[test]
    fn adjacent_wrapped_cycles_both_directions() {
        let catalog = FolderCatalog::from_files(
            ["a.png", "b.png", "c.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            Some(Path::new("c.png")),
            MediaKind::Image,
        );
        assert_eq!(catalog.adjacent_wrapped(1), Some(PathBuf::from("a.png")));
        let first = FolderCatalog {
            index: Some(0),
            ..catalog
        };
        assert_eq!(first.adjacent_wrapped(-1), Some(PathBuf::from("c.png")));
    }

    #[test]
    fn adjacent_wrapped_does_not_reload_single_image() {
        let catalog = FolderCatalog::from_files(
            vec![PathBuf::from("only.png")],
            Some(Path::new("only.png")),
            MediaKind::Image,
        );

        assert!(catalog.adjacent_wrapped(1).is_none());
        assert!(catalog.adjacent_wrapped(-1).is_none());
    }

    #[test]
    fn large_catalog_cursors_and_removals_are_independent() {
        let paths = (0..100_000)
            .map(|i| PathBuf::from(format!("C:/photos/image{i:06}.png")))
            .collect();
        let mut first = FolderCatalog::from_files(
            paths,
            Some(Path::new("C:/photos/image099999.png")),
            MediaKind::Image,
        );
        let mut second = first.clone();
        second.update_index(Path::new("C:/PHOTOS/IMAGE050000.PNG"));
        assert_eq!(first.index, Some(99999));
        assert_eq!(second.index, Some(50000));
        first.remove(Path::new("C:/photos/image050000.png"));
        assert!(!first.contains(Path::new("C:/photos/image050000.png")));
        assert!(second.contains(Path::new("C:/photos/image050000.png")));
        assert_eq!(
            second.adjacent_wrapped(1),
            Some(PathBuf::from("C:/photos/image050001.png"))
        );
    }

    #[test]
    fn mixed_folder_has_independent_image_and_video_navigation() {
        let folder =
            std::env::temp_dir().join(format!("kgen-mixed-catalog-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let names = ["01.JPG", "02.MP4", "03.png", "04.webm"];
        for name in names {
            fs::write(folder.join(name), []).unwrap();
        }
        let photos = FolderCatalog::scan_cancellable(&folder.join(names[0]), 0, &|| false);
        let videos = FolderCatalog::scan_cancellable(&folder.join(names[1]), 0, &|| false);
        for name in names {
            fs::remove_file(folder.join(name)).unwrap();
        }
        fs::remove_dir(&folder).unwrap();
        assert_eq!(photos.adjacent_wrapped(1), Some(folder.join(names[2])));
        assert_eq!(videos.adjacent_wrapped(1), Some(folder.join(names[3])));
        assert_eq!(photos.first(), Some(folder.join(names[0])));
        assert_eq!(videos.first(), Some(folder.join(names[1])));
        assert!(!photos.matches(&folder.join(names[1]), MediaKind::Video));
    }
}
