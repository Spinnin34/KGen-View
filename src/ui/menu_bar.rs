use windows::{
    Win32::{
        Foundation::POINT,
        Graphics::{
            Direct2D::{Common::D2D1_COLOR_F, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1HwndRenderTarget},
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
            },
        },
    },
    core::w,
};

use crate::i18n::{Language, Text, tr};

use super::layout::ChromeLayout;

use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;

const LABELS: [Text; 6] = [
    Text::File,
    Text::Edit,
    Text::Image,
    Text::View,
    Text::Options,
    Text::Help,
];

#[derive(Default)]
pub struct MenuBarState {
    pub hovered: Option<usize>,
    pub pressed: Option<usize>,
    dwrite_factory: Option<IDWriteFactory>,
    text_format: Option<IDWriteTextFormat>,
    brushes: Option<MenuBrushes>,
}

#[derive(Clone)]
struct MenuBrushes {
    dark: bool,
    foreground: ID2D1SolidColorBrush,
    muted: ID2D1SolidColorBrush,
}

impl MenuBarState {
    pub fn hit_test(&self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        layout
            .menu_items
            .iter()
            .find_map(|item| item.rect.contains(point).then_some(item.index))
    }

    pub fn update_hover(&mut self, layout: &ChromeLayout, point: POINT) -> bool {
        let next = self.hit_test(layout, point);
        let changed = next != self.hovered;
        self.hovered = next;
        changed
    }

    pub fn press(&mut self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        self.pressed = self.hit_test(layout, point);
        self.pressed
    }

    pub fn release(&mut self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        let released = self.hit_test(layout, point);
        let activated = (released == self.pressed).then_some(released).flatten();
        self.pressed = None;
        activated
    }

    pub unsafe fn draw_d2d(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        layout: &ChromeLayout,
        language: Language,
        dark: bool,
    ) {
        if layout.menu.bottom <= layout.menu.top {
            return;
        }

        if self.dwrite_factory.is_none() {
            self.dwrite_factory =
                unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok();
        }
        let Some(factory) = &self.dwrite_factory else {
            return;
        };

        if self.text_format.is_none() {
            let format = unsafe {
                factory.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    12.0 * layout.scale,
                    w!("es-ES"),
                )
            };
            if let Ok(fmt) = format {
                let _ = unsafe { fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) };
                let _ = unsafe { fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER) };
                self.text_format = Some(fmt);
            }
        }
        let Some(text_format) = &self.text_format else {
            return;
        };

        let brushes = if self
            .brushes
            .as_ref()
            .is_some_and(|value| value.dark == dark)
        {
            self.brushes.as_ref().unwrap().clone()
        } else {
            let (fg_color, muted_color) = if dark {
                (
                    D2D1_COLOR_F {
                        r: 0.95,
                        g: 0.95,
                        b: 0.95,
                        a: 1.0,
                    },
                    D2D1_COLOR_F {
                        r: 0.72,
                        g: 0.72,
                        b: 0.72,
                        a: 1.0,
                    },
                )
            } else {
                (
                    D2D1_COLOR_F {
                        r: 0.12,
                        g: 0.12,
                        b: 0.12,
                        a: 1.0,
                    },
                    D2D1_COLOR_F {
                        r: 0.38,
                        g: 0.38,
                        b: 0.38,
                        a: 1.0,
                    },
                )
            };
            let Ok(foreground) = (unsafe { target.CreateSolidColorBrush(&fg_color, None) }) else {
                return;
            };
            let Ok(muted) = (unsafe { target.CreateSolidColorBrush(&muted_color, None) }) else {
                return;
            };
            let brushes = MenuBrushes {
                dark,
                foreground,
                muted,
            };
            self.brushes = Some(brushes.clone());
            brushes
        };

        for item in &layout.menu_items {
            let text = if item.index < LABELS.len() {
                tr(language, LABELS[item.index])
            } else {
                "•••"
            };
            let brush = if self.hovered == Some(item.index) {
                &brushes.foreground
            } else {
                &brushes.muted
            };
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let rect = item.rect.as_d2d();
            unsafe {
                target.DrawText(
                    &utf16,
                    text_format,
                    &rect,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    pub fn reset_render_resources(&mut self) {
        self.brushes = None;
    }
}
