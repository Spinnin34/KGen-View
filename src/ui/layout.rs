use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Direct2D::{Common::D2D_RECT_F, D2D1_ROUNDED_RECT},
    UI::HiDpi::GetDpiForWindow,
};

use super::toolbar::toolbar_items;

const MENU_DIP: f32 = 23.0;
const TOOLBAR_DIP: f32 = 33.0;
const STATUS_DIP: f32 = 20.0;
const BUTTON_DIP: f32 = 29.0;
const GROUP_GAP_DIP: f32 = 4.0;
const EDGE_DIP: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PxRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl PxRect {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn contains(self, point: POINT) -> bool {
        (point.x as f32) >= self.left
            && (point.x as f32) < self.right
            && (point.y as f32) >= self.top
            && (point.y as f32) < self.bottom
    }

    pub fn inset(self, value: f32) -> Self {
        Self::new(
            self.left + value,
            self.top + value,
            self.right - value,
            self.bottom - value,
        )
    }

    pub fn as_d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    pub fn as_rounded(self, radius: f32) -> D2D1_ROUNDED_RECT {
        D2D1_ROUNDED_RECT {
            rect: self.as_d2d(),
            radiusX: radius,
            radiusY: radius,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedToolbarItem {
    pub index: usize,
    pub rect: PxRect,
    pub group_rect: PxRect,
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedMenuItem {
    /// 0..=5 are the menu categories; 6 is the compact overflow button.
    pub index: usize,
    pub rect: PxRect,
}

#[derive(Clone, Debug)]
pub struct ChromeLayout {
    pub scale: f32,
    pub menu: PxRect,
    pub menu_bottom_rule: PxRect,
    pub toolbar: PxRect,
    pub toolbar_bottom_rule: PxRect,
    pub image: PxRect,
    pub status: PxRect,
    pub status_top_rule: PxRect,
    pub toolbar_items: Vec<PlacedToolbarItem>,
    pub menu_items: Vec<PlacedMenuItem>,
}

impl ChromeLayout {
    pub fn calculate(hwnd: HWND, client: RECT, fullscreen: bool, update_available: bool) -> Self {
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let scale = dpi as f32 / 96.0;
        let width = client.right.max(1) as f32;
        let height = client.bottom.max(1) as f32;
        let menu_height = if fullscreen { 0.0 } else { px(MENU_DIP, scale) };
        let toolbar_height = if fullscreen {
            0.0
        } else {
            px(TOOLBAR_DIP, scale)
        };
        let status_height = if fullscreen {
            0.0
        } else {
            px(STATUS_DIP, scale)
        };
        let rule = scale.max(1.0);

        let menu = PxRect::new(0.0, 0.0, width, menu_height);
        let menu_bottom_rule = PxRect::new(0.0, menu_height - rule, width, menu_height);
        let toolbar = PxRect::new(0.0, menu_height, width, menu_height + toolbar_height);
        let toolbar_bottom_rule = PxRect::new(0.0, toolbar.bottom - rule, width, toolbar.bottom);
        let status = PxRect::new(0.0, height - status_height, width, height);
        let status_top_rule = PxRect::new(0.0, status.top, width, status.top + rule);
        let image = PxRect::new(0.0, toolbar.bottom, width, status.top.max(toolbar.bottom));

        let mut menu_items = Vec::new();
        if !fullscreen {
            // Widths are intentionally fixed and compact. Narrow windows keep Archivo
            // plus an overflow menu instead of wrapping or crushing the image viewport.
            let widths = [57.0, 53.0, 62.0, 43.0, 67.0, 52.0];
            let overflow = px(28.0, scale);
            let edge = px(4.0, scale);
            let mut x = edge;
            for (index, dip_width) in widths.into_iter().enumerate() {
                let item_width = px(dip_width, scale);
                let remaining_after = widths.len() - index - 1;
                let reserve = if remaining_after > 0 {
                    overflow + edge
                } else {
                    edge
                };
                if x + item_width + reserve > width {
                    menu_items.push(PlacedMenuItem {
                        index: 6,
                        rect: PxRect::new(
                            x,
                            2.0 * scale,
                            (x + overflow).min(width - edge),
                            menu_height - 2.0 * scale,
                        ),
                    });
                    break;
                }
                menu_items.push(PlacedMenuItem {
                    index,
                    rect: PxRect::new(x, 1.0 * scale, x + item_width, menu_height - 1.0 * scale),
                });
                x += item_width;
            }
        }

        let mut placed = Vec::new();
        if !fullscreen {
            let button = px(BUTTON_DIP, scale);
            let gap = px(GROUP_GAP_DIP, scale);
            let edge = px(EDGE_DIP, scale);
            let mut x = edge;
            let icons_right = width - edge;
            let visible_items: Vec<_> = toolbar_items()
                .iter()
                .enumerate()
                .filter(|(_, item)| item.command != crate::commands::UPDATE || update_available)
                .collect();
            let mut cursor = 0;
            while cursor < visible_items.len() {
                let group = visible_items[cursor].1.group;
                let start = cursor;
                while cursor < visible_items.len() && visible_items[cursor].1.group == group {
                    cursor += 1;
                }
                let group_left = x;
                let group_right = (x + button * (cursor - start) as f32).min(width - edge);
                let group_rect = PxRect::new(
                    group_left,
                    menu_height + 2.0 * scale,
                    group_right.min(icons_right),
                    toolbar.bottom - 3.0 * scale,
                );
                let mut group_fits = true;
                for index in start..cursor {
                    let right = x + button;
                    if right > icons_right {
                        group_fits = false;
                        break;
                    }
                    placed.push(PlacedToolbarItem {
                        index: visible_items[index].0,
                        rect: PxRect::new(
                            x,
                            menu_height + 2.0 * scale,
                            right,
                            toolbar.bottom - 3.0 * scale,
                        ),
                        group_rect,
                    });
                    x += button;
                }
                if !group_fits {
                    break;
                }
                x += gap;
                if x >= icons_right {
                    break;
                }
            }
        }

        Self {
            scale,
            menu,
            menu_bottom_rule,
            toolbar,
            toolbar_bottom_rule,
            image,
            status,
            status_top_rule,
            toolbar_items: placed,
            menu_items,
        }
    }

    pub fn toolbar_height(&self) -> i32 {
        (self.menu.bottom + self.toolbar.bottom - self.toolbar.top).round() as i32
    }

    pub fn status_height(&self) -> i32 {
        (self.status.bottom - self.status.top).round() as i32
    }
}

fn px(dip: f32, scale: f32) -> f32 {
    (dip * scale).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_hit_test_excludes_right_and_bottom_edges() {
        let rect = PxRect::new(0.0, 0.0, 10.0, 10.0);
        assert!(rect.contains(POINT { x: 9, y: 9 }));
        assert!(!rect.contains(POINT { x: 10, y: 10 }));
    }
}
