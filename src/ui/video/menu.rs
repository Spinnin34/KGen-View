use super::*;
use windows::Win32::{
    Graphics::{
        Direct2D::ID2D1Factory, DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING, Gdi::ClientToScreen,
    },
    UI::WindowsAndMessaging::GetClientRect,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPage {
    Playback,
    Speed,
    Tracks,
    Audio,
    Subtitles,
    Chapters,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuCommand {
    Page(MenuPage),
    Rate(f32),
    Loop,
    NextFrame,
    PreviousPosition,
    Volume(i32),
    Mute,
    Audio(i32),
    Subtitle(i32),
    Chapter(i64),
    File(usize),
    ApplicationMenu,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuRow {
    pub label: String,
    pub detail: String,
    pub command: MenuCommand,
    pub selected: bool,
}

impl MenuRow {
    pub fn new(label: impl Into<String>, detail: impl Into<String>, command: MenuCommand) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            command,
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuHit {
    Back,
    Close,
    Row(usize),
}

#[derive(Clone, Copy, PartialEq)]
struct MenuLayout {
    origin: POINT,
    size: SIZE,
    scale: f32,
    visible_rows: usize,
}

impl MenuLayout {
    fn new(client: RECT, dock: &PanelLayout, row_count: usize) -> Option<Self> {
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width < 96 || height < 100 {
            return None;
        }
        let scale = dock.scale.min(width as f32 / 270.0);
        let inset = (8.0 * scale).round() as i32;
        let mut bottom = dock.body_top() - inset;
        if bottom - client.top < (120.0 * scale).round() as i32 {
            bottom = client.bottom - inset;
        }
        let available = (bottom - client.top - inset) as f32 / scale;
        let visible_rows = (((available - 50.0) / 33.0).floor() as usize)
            .max(1)
            .min(row_count.max(1));
        let size = SIZE {
            cx: (308.0 * scale).round().min((width - inset * 2) as f32) as i32,
            cy: ((50 + visible_rows * 33) as f32 * scale).round() as i32,
        };
        Some(Self {
            origin: POINT {
                x: (dock.offset.x + dock.size.cx - size.cx).max(client.left + inset),
                y: bottom - size.cy,
            },
            size,
            scale,
            visible_rows,
        })
    }

    fn hit(self, point: POINT, first: usize, count: usize, back: bool) -> Option<MenuHit> {
        let x = point.x as f32;
        let y = point.y as f32 / self.scale;
        if x < 0.0 || x >= self.size.cx as f32 || y < 0.0 || point.y >= self.size.cy {
            return None;
        }
        if y < 43.0 {
            if x >= self.size.cx as f32 - 42.0 * self.scale {
                return Some(MenuHit::Close);
            }
            return (back && x < 41.0 * self.scale).then_some(MenuHit::Back);
        }
        let index = ((y - 43.0) / 33.0) as usize;
        (index < self.visible_rows && index + first < count).then_some(MenuHit::Row(index + first))
    }
}

pub struct VideoMenu {
    layer: Layer,
    painter: Option<MenuPainter>,
    pub page: Option<MenuPage>,
    root: MenuPage,
    title: &'static str,
    rows: Vec<MenuRow>,
    layout: Option<MenuLayout>,
    screen: POINT,
    first: usize,
    focus: Option<usize>,
    hover: Option<MenuHit>,
    pressed: Option<MenuHit>,
    dirty: bool,
}

impl VideoMenu {
    pub fn new(owner: windows::Win32::Foundation::HWND, factory: &ID2D1Factory) -> Result<Self> {
        Ok(Self {
            layer: Layer::new(owner, factory, LayerKind::Menu)?,
            painter: None,
            root: MenuPage::Playback,
            page: None,
            title: "",
            rows: Vec::new(),
            layout: None,
            screen: POINT::default(),
            first: 0,
            focus: None,
            hover: None,
            pressed: None,
            dirty: true,
        })
    }

    pub fn open(&mut self, page: MenuPage) {
        self.root = page;
        self.change_page(page);
    }

    pub fn change_page(&mut self, page: MenuPage) {
        self.page = Some(page);
        self.first = 0;
        self.focus = None;
        self.hover = None;
        self.pressed = None;
        self.dirty = true;
    }

    pub fn back(&mut self) {
        self.change_page(self.root);
    }

    pub fn opened(&self) -> bool {
        self.page.is_some()
    }

    pub fn close(&mut self) {
        self.page = None;
        self.layer.hide();
        self.pressed = None;
        self.dirty = true;
    }

    pub fn set_rows(&mut self, title: &'static str, rows: Vec<MenuRow>) {
        if self.title != title || self.rows != rows {
            self.title = title;
            self.rows = rows;
            self.dirty = true;
        }
    }

    pub fn local(&self, client: POINT) -> Option<POINT> {
        let layout = self.layout?;
        let point = POINT {
            x: client.x - layout.origin.x,
            y: client.y - layout.origin.y,
        };
        (self.layer.visible
            && (0..layout.size.cx).contains(&point.x)
            && (0..layout.size.cy).contains(&point.y))
        .then_some(point)
    }

    fn hit(&self, point: POINT) -> Option<MenuHit> {
        self.layout?.hit(
            point,
            self.first,
            self.rows.len(),
            self.page != Some(self.root),
        )
    }

    pub fn hover_at(&mut self, client: POINT, moved: bool) -> bool {
        if !moved {
            return false;
        }
        let hit = self.local(client).and_then(|point| self.hit(point));
        if self.hover == hit && self.focus.is_none() {
            return false;
        }
        self.hover = hit;
        self.focus = None;
        self.dirty = true;
        true
    }

    pub fn press(&mut self, point: POINT) {
        self.pressed = self.hit(point);
    }

    pub fn release(&mut self, point: POINT) -> Option<MenuHit> {
        let hit = self.hit(point);
        self.pressed.take().filter(|pressed| Some(*pressed) == hit)
    }

    pub fn cancel_press(&mut self) {
        self.pressed = None;
    }

    pub fn command_at(&self, row: usize) -> Option<MenuCommand> {
        self.rows.get(row).map(|row| row.command)
    }

    pub fn focused(&self) -> Option<MenuHit> {
        self.focus.map(MenuHit::Row).or(self.hover)
    }

    pub fn move_focus(&mut self, step: isize) {
        let count = self.rows.len();
        if count == 0 {
            return;
        }
        let start = self.focus.or(match self.hover {
            Some(MenuHit::Row(row)) => Some(row),
            _ => None,
        });
        let row = start.map_or_else(
            || if step < 0 { count - 1 } else { 0 },
            |index| (index as isize + step).rem_euclid(count as isize) as usize,
        );
        let visible = self.layout.map_or(1, |layout| layout.visible_rows);
        if row < self.first {
            self.first = row;
        }
        if row >= self.first + visible {
            self.first = row + 1 - visible;
        }
        self.focus = Some(row);
        self.hover = None;
        self.dirty = true;
    }

    pub fn scroll(&mut self, rows: isize) {
        if let Some(layout) = self.layout {
            let maximum = self.rows.len().saturating_sub(layout.visible_rows);
            self.first = self.first.saturating_add_signed(rows).min(maximum);
            self.focus = None;
            self.hover = None;
            self.dirty = true;
        }
    }

    pub fn draw(
        &mut self,
        owner: windows::Win32::Foundation::HWND,
        dock: &PanelLayout,
    ) -> Result<()> {
        let mut client = RECT::default();
        unsafe {
            GetClientRect(owner, &mut client)?;
        }
        let Some(layout) = MenuLayout::new(client, dock, self.rows.len()) else {
            self.close();
            return Ok(());
        };
        let mut origin = layout.origin;
        unsafe {
            let _ = ClientToScreen(owner, &mut origin);
        }
        if !self.dirty && self.layout == Some(layout) && self.screen == origin && self.layer.visible
        {
            return Ok(());
        }
        self.first = self
            .first
            .min(self.rows.len().saturating_sub(layout.visible_rows));
        let layout_changed = self.layout != Some(layout);
        self.layout = Some(layout);
        self.screen = origin;
        if layout_changed {
            self.hover = None;
        }
        let highlight = self.focused();
        let back = self.page != Some(self.root);
        let painter = &mut self.painter;
        self.layer.render(origin, layout.size, |target| {
            if painter
                .as_ref()
                .is_none_or(|painter| painter.scale != layout.scale)
            {
                *painter = Some(MenuPainter::new(target, layout.scale)?);
            }
            painter.as_ref().unwrap().draw(
                target, layout, self.title, &self.rows, self.first, highlight, back,
            );
            Ok(())
        })?;
        self.dirty = false;
        Ok(())
    }
}

struct MenuPainter {
    scale: f32,
    title: IDWriteTextFormat,
    text: IDWriteTextFormat,
    detail: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
}

impl MenuPainter {
    fn new(target: &ID2D1DCRenderTarget, scale: f32) -> Result<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let title = text_format(
            &factory,
            w!("Segoe UI Semibold"),
            14.0 * scale,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        )?;
        Ok(Self {
            scale,
            title,
            text: text_format(
                &factory,
                w!("Segoe UI"),
                12.5 * scale,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            detail: text_format(
                &factory,
                w!("Segoe UI"),
                11.5 * scale,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            )?,
            icons: text_format(
                &factory,
                w!("Segoe MDL2 Assets"),
                11.0 * scale,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            brush: unsafe { target.CreateSolidColorBrush(&INK, None)? },
        })
    }

    fn draw(
        &self,
        target: &ID2D1DCRenderTarget,
        layout: MenuLayout,
        title: &str,
        rows: &[MenuRow],
        first: usize,
        highlight: Option<MenuHit>,
        back: bool,
    ) {
        let scale = layout.scale;
        let width = layout.size.cx as f32;
        let height = layout.size.cy as f32;
        let brush = &self.brush;
        let fill = |rect: PxRect, color: D2D1_COLOR_F, radius: f32| unsafe {
            brush.SetColor(&color);
            target.FillRoundedRectangle(&rect.as_rounded(radius * scale), brush);
        };
        let text = |value: &[u16], region: PxRect, font: &IDWriteTextFormat, color| unsafe {
            brush.SetColor(&color);
            target.DrawText(
                value,
                font,
                &region.as_d2d(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        };
        let r = |left, top, right, bottom| {
            PxRect::new(left * scale, top * scale, right * scale, bottom * scale)
        };
        let right = width / scale;
        let bounds = PxRect::new(0.5, 0.5, width - 0.5, height - 0.5);
        fill(bounds, rgb(0x202024), 8.0);
        unsafe {
            brush.SetColor(&HAIRLINE);
            target.DrawRoundedRectangle(&bounds.as_rounded(8.0 * scale), brush, 1.0, None);
        }
        fill(r(12.0, 40.0, right - 12.0, 40.8), HAIRLINE, 0.0);
        let close = r(right - 37.0, 7.0, right - 7.0, 36.0);
        if highlight == Some(MenuHit::Close) {
            fill(close, HOVER, 5.0);
        }
        text(&[0xe711], close, &self.icons, MUTED);
        if back {
            let back = r(7.0, 7.0, 37.0, 36.0);
            if highlight == Some(MenuHit::Back) {
                fill(back, HOVER, 5.0);
            }
            text(&[0xe72b], back, &self.icons, MUTED);
        }
        text(
            &title.encode_utf16().collect::<Vec<_>>(),
            r(if back { 42.0 } else { 15.0 }, 4.0, right - 40.0, 39.0),
            &self.title,
            INK,
        );
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(first)
            .take(layout.visible_rows)
        {
            let top = 43.0 + (index - first) as f32 * 33.0;
            let bottom = top + 31.0;
            let rect = r(7.0, top, right - 8.0, bottom);
            if highlight == Some(MenuHit::Row(index)) {
                fill(rect, HOVER, 5.0);
            } else if row.selected {
                fill(rect, rgb(0x342a26), 5.0);
            }
            let tail = if row.detail.is_empty() {
                right - 36.0
            } else {
                right - 83.0
            };
            text(
                &row.label.encode_utf16().collect::<Vec<_>>(),
                r(15.0, top, tail, bottom),
                &self.text,
                if row.selected { HOT_ACCENT } else { INK },
            );
            if !row.detail.is_empty() {
                text(
                    &row.detail.encode_utf16().collect::<Vec<_>>(),
                    r(tail + 2.0, top, right - 35.0, bottom),
                    &self.detail,
                    MUTED,
                );
            }
            let glyph = if row.selected {
                Some(0xe73e)
            } else if matches!(row.command, MenuCommand::Page(_)) {
                Some(0xe76c)
            } else {
                None
            };
            if let Some(glyph) = glyph {
                text(
                    &[glyph],
                    r(right - 31.0, top, right - 13.0, bottom),
                    &self.icons,
                    if row.selected { ACCENT } else { MUTED },
                );
            }
        }
        if rows.len() > layout.visible_rows {
            let top = 44.0;
            let track = layout.visible_rows as f32 * 33.0 - 3.0;
            let thumb_top = top + track * first as f32 / rows.len() as f32;
            let thumb_bottom = thumb_top + track * layout.visible_rows as f32 / rows.len() as f32;
            fill(
                r(right - 4.0, thumb_top, right - 2.0, thumb_bottom),
                TRACK,
                1.0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_stays_in_the_client_and_hits_the_visible_rows() {
        for dpi in [1.0, 1.5, 2.0] {
            for (width, height) in [(100, 100), (280, 200), (880, 500), (3840, 2160)] {
                let client = RECT {
                    left: 5,
                    top: 7,
                    right: width + 5,
                    bottom: height + 7,
                };
                let dock = PanelLayout::new(client, dpi, false).unwrap();
                let menu = MenuLayout::new(client, &dock, 32).unwrap();
                assert!(menu.origin.x >= client.left && menu.origin.y >= client.top);
                assert!(menu.origin.x + menu.size.cx <= client.right);
                assert!(menu.origin.y + menu.size.cy <= client.bottom);
                let row = POINT {
                    x: menu.size.cx / 2,
                    y: (53.0 * menu.scale).round() as i32,
                };
                assert_eq!(menu.hit(row, 10, 32, true), Some(MenuHit::Row(10)));
                assert_eq!(menu.hit(row, 32, 32, true), None);
            }
        }
    }
}
