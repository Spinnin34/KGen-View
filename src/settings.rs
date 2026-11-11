use windows::{
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
            RegSetKeyValueW,
        },
    },
    core::PCWSTR,
};

use crate::i18n::Language;

const KEY: &str = "Software\\KGenView";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub language: Language,
    pub theme: ThemeMode,
    pub auto_resize: bool,
    pub always_on_top: bool,
    pub slideshow_ms: u32,
    pub confirm_delete: bool,
    pub imgbb_api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::detect(),
            theme: ThemeMode::System,
            auto_resize: true,
            always_on_top: false,
            slideshow_ms: 3000,
            confirm_delete: true,
            imgbb_api_key: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let defaults = Self::default();
        Self {
            language: match read_dword("Language") {
                Some(1) => Language::English,
                Some(0) => Language::Spanish,
                _ => defaults.language,
            },
            theme: match read_dword("Theme") {
                Some(1) => ThemeMode::Light,
                Some(2) => ThemeMode::Dark,
                _ => ThemeMode::System,
            },
            auto_resize: read_dword("AutoResize").map(|v| v != 0).unwrap_or(true),
            always_on_top: read_dword("AlwaysOnTop").map(|v| v != 0).unwrap_or(false),
            slideshow_ms: read_dword("SlideshowMs").unwrap_or(3000).clamp(500, 60_000),
            confirm_delete: read_dword("ConfirmDelete").map(|v| v != 0).unwrap_or(true),
            imgbb_api_key: read_string("ImgBbApiKey").unwrap_or_default(),
        }
    }

    pub fn save(&self) {
        write_dword(
            "Language",
            if self.language == Language::English {
                1
            } else {
                0
            },
        );
        write_dword(
            "Theme",
            match self.theme {
                ThemeMode::System => 0,
                ThemeMode::Light => 1,
                ThemeMode::Dark => 2,
            },
        );
        write_dword("AutoResize", self.auto_resize as u32);
        write_dword("AlwaysOnTop", self.always_on_top as u32);
        write_dword("SlideshowMs", self.slideshow_ms);
        write_dword("ConfirmDelete", self.confirm_delete as u32);
        write_string("ImgBbApiKey", &self.imgbb_api_key);
    }

    pub fn is_dark(&self) -> bool {
        match self.theme {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_uses_dark_mode(),
        }
    }

    pub fn cycle_theme(&mut self) {
        self.theme = match self.theme {
            ThemeMode::System => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::System,
        };
        self.save();
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn read_dword(name: &str) -> Option<u32> {
    read_dword_at(KEY, name)
}

fn read_dword_at(key: &str, name: &str) -> Option<u32> {
    let key = wide(key);
    let name = wide(name);
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    (result == ERROR_SUCCESS).then_some(value)
}

fn write_dword(name: &str, value: u32) {
    let key = wide(KEY);
    let name = wide(name);
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_DWORD.0,
            Some((&value as *const u32).cast()),
            std::mem::size_of::<u32>() as u32,
        );
    }
}

fn read_string(name: &str) -> Option<String> {
    let key = wide(KEY);
    let name = wide(name);
    let mut size = 0u32;
    let first = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if first != ERROR_SUCCESS || size < 2 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let second = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if second != ERROR_SUCCESS {
        return None;
    }
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

fn write_string(name: &str, value: &str) {
    let key = wide(KEY);
    let name = wide(name);
    let value = wide(value);
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(value.as_ptr().cast()),
            (value.len() * 2) as u32,
        );
    }
}

fn system_uses_dark_mode() -> bool {
    read_dword_at(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        "AppsUseLightTheme",
    )
    .map(|value| value == 0)
    .unwrap_or(false)
}
