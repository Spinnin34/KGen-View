//! GPU-rendered application chrome. There are deliberately no Win32 `BUTTON`
//! children: the toolbar, status line and their interaction states share one
//! compact visual system and follow the same DPI scale as the image viewport.

pub mod icons;
pub mod layout;
pub mod menu_bar;
pub mod status;
pub mod theme;
pub mod toolbar;

use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};

use self::{
    layout::{ChromeLayout, PxRect},
    menu_bar::MenuBarState,
    theme::Palette,
    toolbar::{ToolbarState, toolbar_items},
};

pub struct Chrome {
    pub menu: MenuBarState,
    pub toolbar: ToolbarState,
    icon_atlas: Option<ID2D1Bitmap>,
    brushes: Option<ChromeBrushes>,
}

#[derive(Clone)]
struct ChromeBrushes {
    dark: bool,
    chrome: ID2D1SolidColorBrush,
    raised: ID2D1SolidColorBrush,
    hover: ID2D1SolidColorBrush,
    divider: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
}

impl Chrome {
    pub fn new() -> Self {
        Self {
            menu: MenuBarState::default(),
            toolbar: ToolbarState::default(),
            icon_atlas: None,
            brushes: None,
        }
    }

    pub unsafe fn draw(
        &mut self,
        target: &ID2D1HwndRenderTarget,
        layout: &ChromeLayout,
        palette: Palette,
        active_command: Option<usize>,
        language: crate::i18n::Language,
        dark: bool,
    ) {
        if self.icon_atlas.is_none() {
            self.icon_atlas = unsafe { icons::create_atlas(target) }.ok();
        }
        let brushes = if self
            .brushes
            .as_ref()
            .is_some_and(|value| value.dark == dark)
        {
            self.brushes.as_ref().unwrap().clone()
        } else {
            let brush = |color| unsafe { target.CreateSolidColorBrush(&color, None) };
            let Ok(chrome) = brush(palette.chrome) else {
                return;
            };
            let Ok(raised) = brush(palette.raised) else {
                return;
            };
            let Ok(hover) = brush(palette.hover) else {
                return;
            };
            let Ok(divider) = brush(palette.divider) else {
                return;
            };
            let Ok(accent) = brush(palette.accent) else {
                return;
            };
            let brushes = ChromeBrushes {
                dark,
                chrome,
                raised,
                hover,
                divider,
                accent,
            };
            self.brushes = Some(brushes.clone());
            brushes
        };

        unsafe {
            target.FillRectangle(&layout.toolbar.as_d2d(), &brushes.chrome);
            target.FillRectangle(&layout.menu.as_d2d(), &brushes.chrome);
            target.FillRectangle(&layout.status.as_d2d(), &brushes.chrome);
            target.FillRectangle(&layout.menu_bottom_rule.as_d2d(), &brushes.divider);
            target.FillRectangle(&layout.toolbar_bottom_rule.as_d2d(), &brushes.divider);
            target.FillRectangle(&layout.status_top_rule.as_d2d(), &brushes.divider);
        }

        for item in &layout.menu_items {
            if self.menu.hovered == Some(item.index) || self.menu.pressed == Some(item.index) {
                unsafe {
                    target
                        .FillRoundedRectangle(&item.rect.inset(1.0).as_rounded(4.0), &brushes.hover)
                };
            }
        }

        unsafe {
            self.menu.draw_d2d(target, layout, language, dark);
        }

        let mut previous_group = None;
        for placed in layout.toolbar_items.iter() {
            let item = &toolbar_items()[placed.index];
            if previous_group != Some(item.group) {
                unsafe {
                    target.FillRoundedRectangle(&placed.group_rect.as_rounded(3.0), &brushes.raised)
                };
                previous_group = Some(item.group);
            }
            if self.toolbar.hovered == Some(item.command)
                || self.toolbar.pressed == Some(item.command)
            {
                unsafe {
                    target.FillRoundedRectangle(
                        &placed.rect.inset(2.0).as_rounded(3.0),
                        &brushes.hover,
                    )
                };
            }
            if active_command == Some(item.command) {
                let marker = PxRect::new(
                    placed.rect.left + 6.0,
                    placed.rect.bottom - 2.0,
                    placed.rect.right - 6.0,
                    placed.rect.bottom,
                );
                unsafe { target.FillRectangle(&marker.as_d2d(), &brushes.accent) };
            }
            if let Some(atlas) = &self.icon_atlas {
                unsafe { icons::draw(target, item.icon, placed.rect, atlas) };
            }
        }
    }

    pub fn reset_render_resources(&mut self) {
        self.icon_atlas = None;
        self.brushes = None;
        self.menu.reset_render_resources();
    }

    pub fn label_for_hover(&self, _language: crate::i18n::Language) -> Option<&'static str> {
        let command = self.toolbar.hovered?;
        toolbar_items()
            .iter()
            .find(|item| item.command == command)
            .map(|item| item.label)
    }

    pub fn accent_brush(&self) -> Option<&ID2D1SolidColorBrush> {
        self.brushes.as_ref().map(|b| &b.accent)
    }
}
