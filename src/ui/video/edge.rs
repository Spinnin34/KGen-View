use super::*;
use crate::platform::playback::MAX_VOLUME;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeZone {
    #[default]
    None,
    Bottom,
    Side,
}

#[derive(Clone, Copy)]
pub struct EdgeLayout {
    client: RECT,
    bottom: i32,
    side: i32,
    inset: i32,
    scale: f32,
}

impl EdgeLayout {
    pub fn new(client: RECT, scale: f32, panel: Option<&PanelLayout>) -> Self {
        let height = (client.bottom - client.top).max(0);
        let bottom = panel
            .map(|panel| panel.offset.y - (17.0 * panel.scale).round() as i32)
            .unwrap_or(client.bottom)
            .max(client.top + height * 3 / 5);
        let width = (client.right - client.left).max(0);
        let inset = panel.map_or_else(
            || horizontal_inset(width, scale),
            |panel| panel.offset.x - client.left,
        );
        let side = client.right - (inset + (62.0 * scale).round() as i32).min(width * 2 / 5);
        Self {
            client,
            bottom,
            side,
            inset,
            scale,
        }
    }

    pub fn zone(self, point: POINT) -> EdgeZone {
        if !(self.client.left..self.client.right).contains(&point.x)
            || !(self.client.top..self.client.bottom).contains(&point.y)
        {
            EdgeZone::None
        } else if point.y >= self.bottom {
            EdgeZone::Bottom
        } else if point.x >= self.side {
            EdgeZone::Side
        } else {
            EdgeZone::None
        }
    }

    pub fn progress(self) -> Option<(POINT, SIZE)> {
        let width = self.client.right - self.client.left;
        let height = (2.0 * self.scale).round() as i32;
        (width > 0 && self.client.bottom - self.client.top >= height).then_some((
            POINT {
                x: self.client.left,
                y: self.client.bottom - height,
            },
            SIZE {
                cx: width,
                cy: height,
            },
        ))
    }

    pub fn volume(self) -> Option<VolumeLayout> {
        let client = self.client;
        let height = client.bottom - client.top;
        let width = client.right - client.left;
        if height < 92 || width < 100 {
            return None;
        }
        let room = self.bottom - client.top;
        let scale = self
            .scale
            .min(room as f32 / 174.0)
            .min(width as f32 / 160.0);
        let inset = self.inset;
        let size = SIZE {
            cx: (50.0 * scale).round() as i32,
            cy: (158.0 * scale).round() as i32,
        };
        let vertical_inset = inset.min((room - size.cy) / 2);
        let offset = POINT {
            x: client.right - size.cx - inset,
            y: (client.top + (height - size.cy) / 2).clamp(
                client.top + vertical_inset,
                client.top + room - size.cy - vertical_inset,
            ),
        };
        Some(VolumeLayout {
            offset,
            size,
            scale,
            track: PxRect::new(
                6.0 * scale,
                (32.0 * scale).round(),
                44.0 * scale,
                (120.0 * scale).round(),
            ),
            mute: PxRect::new(8.0 * scale, 128.0 * scale, 42.0 * scale, 153.0 * scale),
        })
    }
}

pub struct ProgressLine {
    layer: Layer,
    brush: Option<ID2D1SolidColorBrush>,
    previous: Option<(POINT, SIZE, i32)>,
}

impl ProgressLine {
    pub fn new(layer: Layer) -> Self {
        Self {
            layer,
            brush: None,
            previous: None,
        }
    }

    pub fn draw(&mut self, pos: POINT, size: SIZE, status: PlaybackStatus) -> Result<()> {
        let Some(fraction) = progress(status) else {
            self.hide();
            return Ok(());
        };
        let played = Self::played_pixels(size, fraction);
        let frame = (pos, size, played);
        if self.layer.visible && self.previous == Some(frame) {
            return Ok(());
        }
        let brush = &mut self.brush;
        self.layer.render(pos, size, |target| unsafe {
            if brush.is_none() {
                *brush = Some(target.CreateSolidColorBrush(&ACCENT, None)?);
            }
            let brush = brush.as_ref().unwrap();
            let rect = PxRect::new(0.0, 0.0, size.cx as f32, size.cy as f32);
            brush.SetColor(&D2D1_COLOR_F { a: 0.55, ..TRACK });
            target.FillRectangle(&rect.as_d2d(), brush);
            if played > 0 {
                brush.SetColor(&ACCENT);
                target.FillRectangle(
                    &PxRect {
                        right: played as f32,
                        ..rect
                    }
                    .as_d2d(),
                    brush,
                );
            }
            Ok(())
        })?;
        self.previous = Some(frame);
        Ok(())
    }

    pub fn is_visible(&self) -> bool {
        self.layer.visible
    }

    pub fn position_changed(&self, status: PlaybackStatus) -> bool {
        self.layer.visible
            && self.previous.is_some_and(|(_, size, played)| {
                progress(status)
                    .is_none_or(|fraction| Self::played_pixels(size, fraction) != played)
            })
    }

    fn played_pixels(size: SIZE, fraction: f32) -> i32 {
        (size.cx as f32 * fraction).round() as i32
    }

    pub fn hide(&mut self) {
        self.layer.hide();
    }

    pub fn reset(&mut self) {
        self.layer.reset();
        self.brush = None;
        self.previous = None;
    }
}

#[derive(Clone, Copy)]
pub struct VolumeLayout {
    pub offset: POINT,
    pub size: SIZE,
    scale: f32,
    track: PxRect,
    mute: PxRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeHit {
    Track,
    Mute,
}

impl VolumeLayout {
    pub fn local(self, point: POINT) -> POINT {
        POINT {
            x: point.x - self.offset.x,
            y: point.y - self.offset.y,
        }
    }

    pub fn contains(self, local: POINT) -> bool {
        (0..self.size.cx).contains(&local.x) && (0..self.size.cy).contains(&local.y)
    }

    pub fn hit(self, local: POINT) -> Option<VolumeHit> {
        if self.track.inset(-4.0 * self.scale).contains(local) {
            Some(VolumeHit::Track)
        } else if self.mute.contains(local) {
            Some(VolumeHit::Mute)
        } else {
            None
        }
    }

    pub fn value(self, y: i32) -> i32 {
        let top = self.track.top.round() as i32;
        let bottom = self.track.bottom.round() as i32;
        if y == self.track_y(100).round() as i32 {
            return 100;
        }
        let fraction = ((bottom - y) as f32 / (bottom - top) as f32).clamp(0.0, 1.0);
        (fraction * MAX_VOLUME as f32).round() as i32
    }

    fn track_y(self, value: i32) -> f32 {
        self.track.bottom - (self.track.bottom - self.track.top) * value as f32 / MAX_VOLUME as f32
    }
}

pub struct VolumePainter {
    scale: f32,
    percent: IDWriteTextFormat,
    icon: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
}

impl VolumePainter {
    pub fn draw(
        painter: &mut Option<Self>,
        target: &ID2D1DCRenderTarget,
        layout: VolumeLayout,
        preferences: Preferences,
        hover: Option<VolumeHit>,
        dragging: bool,
    ) -> Result<()> {
        let scale = layout.scale;
        if painter.as_ref().is_none_or(|value| value.scale != scale) {
            let factory: IDWriteFactory =
                unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
            *painter = Some(Self {
                scale,
                percent: text_format(
                    &factory,
                    w!("Consolas"),
                    12.0 * scale,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                )?,
                icon: text_format(
                    &factory,
                    w!("Segoe MDL2 Assets"),
                    15.0 * scale,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                )?,
                brush: unsafe { target.CreateSolidColorBrush(&INK, None)? },
            });
        }
        let painter = painter.as_ref().unwrap();
        let brush = &painter.brush;
        let fill = |rect: PxRect, radius: f32, color| unsafe {
            brush.SetColor(&color);
            target.FillRoundedRectangle(&rect.as_rounded(radius * scale), brush);
        };
        let label = |text: &[u16], rect: PxRect, font: &IDWriteTextFormat, color| unsafe {
            brush.SetColor(&color);
            target.DrawText(
                text,
                font,
                &rect.as_d2d(),
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        };
        surface(target, brush, layout.size, scale);
        let width = layout.size.cx as f32;
        let amount_color = if preferences.muted {
            MUTED
        } else if preferences.volume > 100 {
            ACCENT
        } else {
            INK
        };
        let volume: Vec<u16> = format!("{}%", preferences.volume).encode_utf16().collect();
        label(
            &volume,
            PxRect::new(0.0, 5.0 * scale, width, 25.0 * scale),
            &painter.percent,
            amount_color,
        );
        let center = width * 0.5;
        let half = 2.35 * scale;
        let rail = PxRect::new(
            center - half,
            layout.track.top,
            center + half,
            layout.track.bottom,
        );
        let knob = layout.track_y(preferences.volume);
        let nominal = layout.track_y(100);
        let active = dragging || hover == Some(VolumeHit::Track);
        let fill_color = if preferences.muted { MUTED } else { ACCENT };
        fill(rail, 2.35, TRACK);
        if knob < rail.bottom {
            let filled = PxRect { top: knob, ..rail };
            let radius = 2.35_f32.min((rail.bottom - knob) * 0.5 / scale);
            fill(filled, radius, fill_color);
            if !preferences.muted && knob < nominal {
                let boosted = PxRect {
                    bottom: nominal,
                    ..filled
                };
                let radius = 2.35_f32.min((nominal - knob) * 0.5 / scale);
                fill(boosted, radius, HOT_ACCENT);
            }
        }
        unsafe {
            brush.SetColor(&MUTED);
            for sign in [-1.0, 1.0] {
                let x = center + sign * 9.5 * scale;
                let mark = PxRect::new(
                    x - 2.2 * scale,
                    nominal - 0.6 * scale,
                    x + 2.2 * scale,
                    nominal + 0.6 * scale,
                );
                target.FillRoundedRectangle(&mark.as_rounded(0.6 * scale), brush);
            }
        }
        draw_thumb(
            target,
            brush,
            center,
            knob,
            scale,
            active,
            dragging,
            preferences.muted,
        );
        if hover == Some(VolumeHit::Mute) {
            fill(layout.mute, 5.0, HOVER);
        }
        label(
            &[speaker_glyph(preferences)],
            layout.mute,
            &painter.icon,
            if preferences.muted { MUTED } else { INK },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_edges_activate_their_controls() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let panel = PanelLayout::new(client, 1.0, false);
        let edge = EdgeLayout::new(client, 1.0, panel.as_ref());
        assert_eq!(edge.zone(POINT { x: 400, y: 300 }), EdgeZone::None);
        assert_eq!(edge.zone(POINT { x: 400, y: 599 }), EdgeZone::Bottom);
        assert_eq!(edge.zone(POINT { x: 790, y: 300 }), EdgeZone::Side);
        assert_eq!(edge.zone(POINT { x: 790, y: 599 }), EdgeZone::Bottom);
        assert_eq!(edge.zone(POINT { x: 800, y: 300 }), EdgeZone::None);
    }

    #[test]
    fn volume_uses_the_full_range_and_fits_a_short_window() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 360,
            bottom: 190,
        };
        let panel = PanelLayout::new(client, 1.0, false);
        let volume = EdgeLayout::new(client, 1.0, panel.as_ref())
            .volume()
            .unwrap();
        assert!(volume.offset.y >= client.top && volume.offset.y + volume.size.cy <= client.bottom);
        assert_eq!(volume.value(-100), 200);
        assert_eq!(volume.value(500), 0);
        assert_eq!(volume.value(volume.track_y(100).round() as i32), 100);
        assert_eq!(volume.value(volume.track.top as i32), MAX_VOLUME);
        assert_eq!(volume.value(volume.track.bottom as i32), 0);
    }

    #[test]
    fn bottom_dock_preserves_margins_and_timeline_endpoints_at_each_width() {
        for dpi in [1.0, 1.5, 2.0] {
            for (width, height) in [(100, 64), (260, 200), (860, 480), (3840, 2160)] {
                let client = RECT {
                    left: 5,
                    top: 7,
                    right: 5 + width,
                    bottom: 7 + height,
                };
                let panel = PanelLayout::new(client, dpi, true).unwrap();
                let margin = panel.offset.x - client.left;
                assert!(margin > 0 && margin * 2 + panel.size.cx == width);
                assert!(panel.size.cx as f32 >= width as f32 * 0.87);
                assert!(
                    panel.offset.y > client.top && panel.offset.y + panel.size.cy < client.bottom
                );
                assert_eq!(panel.fraction(0), 0.0);
                assert_eq!(panel.fraction(panel.size.cx - 1), 1.0);
                assert_eq!(
                    panel.hit(POINT {
                        x: 1,
                        y: panel.size.cy - 1
                    }),
                    Some(Hit::Seek)
                );
                for (action, rect) in &panel.buttons {
                    let point = POINT {
                        x: ((rect.left + rect.right) * 0.5).round() as i32,
                        y: ((rect.top + rect.bottom) * 0.5).round() as i32,
                    };
                    assert_eq!(panel.hit(point), Some(Hit::Button(*action)));
                }
            }
        }
        let small = RECT {
            left: 0,
            top: 0,
            right: 240,
            bottom: 200,
        };
        let full = RECT {
            right: 800,
            bottom: 600,
            ..small
        };
        assert!(
            PanelLayout::new(small, 1.0, false).unwrap().size.cy
                < PanelLayout::new(full, 1.0, false).unwrap().size.cy
        );
    }

    #[test]
    fn overlays_fit_after_resizing_at_common_dpi_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for height in 92..540 {
                let client = RECT {
                    left: 0,
                    top: 0,
                    right: 200,
                    bottom: height,
                };
                let panel = PanelLayout::new(client, scale, false);
                let edges = EdgeLayout::new(client, scale, panel.as_ref());
                assert_eq!(
                    edges.zone(POINT {
                        x: 100,
                        y: height / 2
                    }),
                    EdgeZone::None
                );
                let volume = edges.volume().unwrap();
                assert!(volume.offset.y >= 0 && volume.offset.y + volume.size.cy <= height);
                let (origin, size) = edges.progress().unwrap();
                assert_eq!(origin.y + size.cy, height);
                assert_eq!(size.cx, 200);
            }
        }
    }
}
