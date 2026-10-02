use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

const EXTENSIONS: &[&str] = &[
    "bmp", "dib", "jpg", "jpeg", "jpe", "png", "gif", "tif", "tiff", "ico", "webp", "heic", "heif",
    "jxl", "wdp", "jxr", "hdp", "dds", "svg",
];

#[derive(Default, Debug, Clone)]
pub struct FolderCatalog {
    pub files: Vec<PathBuf>,
    pub index: Option<usize>,
}

impl FolderCatalog {
    pub fn scan(current: &Path) -> Self {
        let Some(folder) = current.parent() else {
            return Self::default();
        };
        let mut entries: Vec<(PathBuf, Option<SystemTime>, String)> = fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if !is_supported(&path) {
                    return None;
                }
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then(|| {
                    let name = display_name(&path);
                    (path, metadata.modified().ok(), name)
                })
            })
            .collect();
        entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| natural_cmp(&a.2, &b.2)));
        let files: Vec<PathBuf> = entries.into_iter().map(|(path, _, _)| path).collect();
        let current_name = current.file_name();
        let index = files.iter().position(|path| {
            path == current || (current_name.is_some() && path.file_name() == current_name)
        });
        Self { files, index }
    }

    pub fn update_index(&mut self, current: &Path) {
        if self.files.is_empty() {
            return;
        }
        let current_name = current.file_name();
        if let Some(pos) = self.files.iter().position(|path| {
            path == current || (current_name.is_some() && path.file_name() == current_name)
        }) {
            self.index = Some(pos);
        }
    }

    pub fn adjacent(&self, offset: isize) -> Option<PathBuf> {
        let index = self.index? as isize + offset;
        if index < 0 || index >= self.files.len() as isize {
            None
        } else {
            Some(self.files[index as usize].clone())
        }
    }

    pub fn adjacent_wrapped(&self, offset: isize) -> Option<PathBuf> {
        if self.files.is_empty() {
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
            let ord = ac
                .to_lowercase()
                .to_string()
                .cmp(&bc.to_lowercase().to_string());
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
        let catalog = FolderCatalog {
            files: ["a.png", "b.png", "c.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            index: Some(2),
        };
        assert_eq!(catalog.adjacent_wrapped(1), Some(PathBuf::from("a.png")));
        let first = FolderCatalog {
            index: Some(0),
            ..catalog
        };
        assert_eq!(first.adjacent_wrapped(-1), Some(PathBuf::from("c.png")));
    }
}
