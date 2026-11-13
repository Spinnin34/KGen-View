use std::{
    cmp::Ordering,
    collections::HashMap,
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
}

impl FolderCatalog {
    pub fn scan_cancellable(current: &Path, should_cancel: &impl Fn() -> bool) -> Self {
        let Some(folder) = current.parent() else {
            return Self::default();
        };
        let Ok(read_dir) = fs::read_dir(folder) else {
            return Self::default();
        };
        let mut entries: Vec<(PathBuf, String)> = Vec::new();
        for entry in read_dir {
            if should_cancel() {
                return Self::default();
            }
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !is_supported(&path) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_file() {
                let name = display_name(&path);
                entries.push((path, name));
            }
        }
        if should_cancel() {
            return Self::default();
        }
        entries.sort_unstable_by(|a, b| natural_cmp(&a.1, &b.1));
        if should_cancel() {
            return Self::default();
        }
        let files: Vec<PathBuf> = entries.into_iter().map(|(path, _)| path).collect();
        Self::from_files(files, Some(current))
    }

    pub fn from_files(files: Vec<PathBuf>, current: Option<&Path>) -> Self {
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
        }
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
        *self = Self::from_files(files, None);
    }

    pub fn update_index(&mut self, current: &Path) {
        self.index = self.positions.get(&path_key(current)).copied();
    }

    pub fn adjacent_wrapped(&self, offset: isize) -> Option<PathBuf> {
        if self.files.len() < 2 {
            return None;
        }
        let current = self.index.unwrap_or(0) as isize;
        let index = (current + offset).rem_euclid(self.files.len() as isize) as usize;
        self.files.get(index).cloned()
    }

    pub fn first(&self) -> Option<PathBuf> {
        self.files.first().cloned()
    }
    pub fn last(&self) -> Option<PathBuf> {
        self.files.last().cloned()
    }
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
    aa.len().cmp(&bb.len()).then_with(|| a.cmp(b))
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
    fn adjacent_wrapped_cycles_both_directions() {
        let catalog = FolderCatalog::from_files(
            ["a.png", "b.png", "c.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            Some(Path::new("c.png")),
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
        let catalog =
            FolderCatalog::from_files(vec![PathBuf::from("only.png")], Some(Path::new("only.png")));

        assert!(catalog.adjacent_wrapped(1).is_none());
        assert!(catalog.adjacent_wrapped(-1).is_none());
    }

    #[test]
    fn large_catalog_cursors_and_removals_are_independent() {
        let paths = (0..100_000)
            .map(|i| PathBuf::from(format!("C:/photos/image{i:06}.png")))
            .collect();
        let mut first =
            FolderCatalog::from_files(paths, Some(Path::new("C:/photos/image099999.png")));
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
}
