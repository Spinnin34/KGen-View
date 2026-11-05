use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND},
        Graphics::Gdi::{
            CreateSolidBrush, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
            FillRect, HBRUSH, HGDIOBJ, SetBkMode, SetTextColor, TRANSPARENT,
        },
        UI::{
            Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED, ODT_MENU},
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetSubMenu, HMENU, MENUINFO,
                MF_OWNERDRAW, MF_POPUP, MF_SEPARATOR, MIM_APPLYTOSUBMENUS, MIM_BACKGROUND,
                SetMenuInfo, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TPM_TOPALIGN, TrackPopupMenuEx,
            },
        },
    },
    core::{Error as WinError, PCWSTR},
};

use crate::{
    commands as cmd,
    i18n::{Language, Text, tr},
    platform::text::wide,
};

struct MenuLabel {
    text: Vec<u16>,
}

pub struct ThemedMenu {
    root: HMENU,
    // Stable heap addresses are required because Win32 stores these pointers in itemData.
    #[allow(clippy::vec_box)]
    labels: Vec<Box<MenuLabel>>,
    background: HBRUSH,
    dark: bool,
    scale: f32,
}

impl ThemedMenu {
    pub fn new(
        hwnd: HWND,
        language: Language,
        dark: bool,
        always_on_top: bool,
    ) -> Result<Self, WinError> {
        let background_color = if dark {
            COLORREF(0x001c1714)
        } else {
            COLORREF(0x00fbfbfb)
        };
        let background = unsafe { CreateSolidBrush(background_color) };
        let root = unsafe { CreatePopupMenu()? };
        let mut menu = Self {
            root,
            labels: Vec::new(),
            background,
            dark,
            scale: unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0,
        };
        unsafe { menu.build(language, always_on_top)? };
        Ok(menu)
    }

    pub unsafe fn show_category(&self, hwnd: HWND, index: usize, x: i32, y: i32) {
        let menu = unsafe { GetSubMenu(self.root, index as i32) };
        if !menu.0.is_null() {
            let _ = unsafe {
                TrackPopupMenuEx(
                    menu,
                    (TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON).0,
                    x,
                    y,
                    hwnd,
                    None,
                )
            };
        }
    }

    pub unsafe fn show_all(&self, hwnd: HWND, x: i32, y: i32) {
        let _ = unsafe {
            TrackPopupMenuEx(
                self.root,
                (TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON).0,
                x,
                y,
                hwnd,
                None,
            )
        };
    }

    unsafe fn build(&mut self, language: Language, always_on_top: bool) -> Result<(), WinError> {
        let file = unsafe { CreatePopupMenu()? };
        unsafe {
            self.item(file, cmd::OPEN, tr(language, Text::Open))?;
            self.item(file, cmd::SAVE_AS, tr(language, Text::SaveAs))?;
            AppendMenuW(file, MF_SEPARATOR, 0, None)?;
            self.item(file, cmd::OPEN_FOLDER, tr(language, Text::OpenFolder))?;
            self.item(file, cmd::COPY_PATH, tr(language, Text::CopyPath))?;
            self.item(file, cmd::DELETE, tr(language, Text::Delete))?;
            AppendMenuW(file, MF_SEPARATOR, 0, None)?;
            self.item(file, cmd::EXIT, tr(language, Text::Exit))?;
            self.popup(self.root, file, tr(language, Text::File))?;

            let edit = CreatePopupMenu()?;
            self.item(edit, cmd::UNDO, tr(language, Text::Undo))?;
            self.item(edit, cmd::REDO, tr(language, Text::Redo))?;
            self.item(edit, cmd::COPY, tr(language, Text::Copy))?;
            self.item(edit, cmd::COPY_URL, "Copiar enlace directo (ImgBB)")?;
            self.item(edit, cmd::CROP, tr(language, Text::Crop))?;
            self.item(
                edit,
                cmd::CONVERT_IMAGE,
                if language == Language::Spanish {
                    "Convertir formato..."
                } else {
                    "Convert image format..."
                },
            )?;
            self.popup(self.root, edit, tr(language, Text::Edit))?;

            let image = CreatePopupMenu()?;
            self.item(image, cmd::ROTATE_LEFT, tr(language, Text::RotateLeft))?;
            self.item(image, cmd::ROTATE_RIGHT, tr(language, Text::RotateRight))?;
            self.item(image, cmd::FLIP_H, tr(language, Text::FlipHorizontal))?;
            self.item(image, cmd::FLIP_V, tr(language, Text::FlipVertical))?;
            self.item(image, cmd::INFO, tr(language, Text::Information))?;
            self.popup(self.root, image, tr(language, Text::Image))?;

            let view = CreatePopupMenu()?;
            self.item(view, cmd::PREVIOUS, tr(language, Text::Previous))?;
            self.item(view, cmd::NEXT, tr(language, Text::Next))?;
            self.item(view, cmd::ACTUAL, tr(language, Text::ActualSize))?;
            self.item(view, cmd::FIT, tr(language, Text::Fit))?;
            self.item(view, cmd::FIT_WIDTH, tr(language, Text::FitWidth))?;
            self.item(view, cmd::FULLSCREEN, tr(language, Text::Fullscreen))?;
            self.item(view, cmd::SLIDESHOW, tr(language, Text::Slideshow))?;
            self.item(
                view,
                cmd::PAUSE_ANIMATION,
                tr(language, Text::PauseAnimation),
            )?;
            self.item(view, cmd::PREVIOUS_FRAME, tr(language, Text::PreviousPage))?;
            self.item(view, cmd::NEXT_FRAME, tr(language, Text::NextPage))?;
            self.item(view, cmd::RELOAD, tr(language, Text::Reload))?;
            self.item(
                view,
                cmd::ALWAYS_ON_TOP,
                if language == Language::Spanish {
                    if always_on_top {
                        "✓ Siempre encima"
                    } else {
                        "Siempre encima"
                    }
                } else if always_on_top {
                    "✓ Always on top"
                } else {
                    "Always on top"
                },
            )?;
            self.popup(self.root, view, tr(language, Text::View))?;

            let options = CreatePopupMenu()?;
            self.item(options, cmd::LANGUAGE, tr(language, Text::LanguageMenu))?;
            self.item(options, cmd::THEME, tr(language, Text::Theme))?;
            self.popup(self.root, options, tr(language, Text::Options))?;

            let help = CreatePopupMenu()?;
            self.item(help, cmd::ABOUT, tr(language, Text::About))?;
            self.popup(self.root, help, tr(language, Text::Help))?;

            let info = MENUINFO {
                cbSize: size_of::<MENUINFO>() as u32,
                fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS,
                hbrBack: self.background,
                ..Default::default()
            };
            SetMenuInfo(self.root, &info)?;
        }
        Ok(())
    }

    unsafe fn item(&mut self, menu: HMENU, id: usize, value: &str) -> Result<(), WinError> {
        let pointer = self.store(value);
        unsafe { AppendMenuW(menu, MF_OWNERDRAW, id, PCWSTR(pointer.cast())) }
    }

    unsafe fn popup(&mut self, root: HMENU, popup: HMENU, value: &str) -> Result<(), WinError> {
        let pointer = self.store(value);
        unsafe {
            AppendMenuW(
                root,
                MF_POPUP | MF_OWNERDRAW,
                popup.0 as usize,
                PCWSTR(pointer.cast()),
            )
        }
    }

    fn store(&mut self, value: &str) -> *const MenuLabel {
        let label = Box::new(MenuLabel { text: wide(value) });
        let pointer = &*label as *const MenuLabel;
        self.labels.push(label);
        pointer
    }

    pub unsafe fn measure(&self, value: *mut MEASUREITEMSTRUCT) -> bool {
        let Some(measure) = (unsafe { value.as_mut() }) else {
            return false;
        };
        if measure.CtlType != ODT_MENU || measure.itemData == 0 {
            return false;
        }
        let label = unsafe { &*(measure.itemData as *const MenuLabel) };
        let count = label.text.len().saturating_sub(1) as u32;
        measure.itemWidth =
            ((count.saturating_mul(7).saturating_add(22)) as f32 * self.scale) as u32;
        measure.itemHeight = (23.0 * self.scale).round() as u32;
        true
    }

    pub unsafe fn draw(&self, value: *mut DRAWITEMSTRUCT) -> bool {
        let Some(draw) = (unsafe { value.as_mut() }) else {
            return false;
        };
        if draw.CtlType != ODT_MENU || draw.itemData == 0 {
            return false;
        }
        let label = unsafe { &*(draw.itemData as *const MenuLabel) };
        let selected = (draw.itemState.0 & ODS_SELECTED.0) != 0;
        let fill_color = if selected {
            if self.dark {
                COLORREF(0x00352b25)
            } else {
                COLORREF(0x00eae4df)
            }
        } else if self.dark {
            COLORREF(0x001c1714)
        } else {
            COLORREF(0x00fbfbfb)
        };
        let fill = unsafe { CreateSolidBrush(fill_color) };
        unsafe { FillRect(draw.hDC, &draw.rcItem, fill) };
        unsafe {
            let _ = DeleteObject(HGDIOBJ(fill.0));
        }

        let foreground = if self.dark {
            COLORREF(0x00f1ebe8)
        } else {
            COLORREF(0x00201a17)
        };
        unsafe {
            SetBkMode(draw.hDC, TRANSPARENT);
            SetTextColor(draw.hDC, foreground);
        }
        let mut rect = draw.rcItem;
        rect.left += (10.0 * self.scale).round() as i32;
        rect.right -= (8.0 * self.scale).round() as i32;
        let count = label.text.len().saturating_sub(1);
        let mut text = label.text[..count].to_vec();
        unsafe {
            DrawTextW(
                draw.hDC,
                &mut text,
                &mut rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            )
        };
        true
    }
}

impl Drop for ThemedMenu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.root);
            let _ = DeleteObject(HGDIOBJ(self.background.0));
        }
    }
}
