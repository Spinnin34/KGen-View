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
        let side = client.right - ((104.0 * scale).round() as i32).min(width * 2 / 5);
        Self {
            client,
            bottom,
            side,
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
        let height = (3.0 * self.scale).round() as i32;
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
        let above_dock = self.bottom - client.top;
        let room = if above_dock >= (124.0 * self.scale).round() as i32 {
            above_dock
        } else {
            height
        };
        let scale = self
            .scale
            .min(room as f32 / 240.0)
            .min(width as f32 / 168.0);
        let inset = (13.0 * scale).round() as i32;
        let size = SIZE {
            cx: (64.0 * scale).round() as i32,
            cy: (214.0 * scale).round() as i32,
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
            track: PxRect::new(9.0 * scale, 42.0 * scale, 55.0 * scale, 169.0 * scale),
            mute: PxRect::new(10.0 * scale, 178.0 * scale, 54.0 * scale, 208.0 * scale),
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
        let played = (size.cx as f32 * fraction).round() as i32;
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
            brush.SetColor(&D2D1_COLOR_F {
                a: 0.53,
                ..rgb(0x2e231d)
            });
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
                brush.SetColor(&HOT_ACCENT);
                target.FillRectangle(
                    &PxRect {
                        left: (played - 2 * size.cy).max(0) as f32,
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
        let fraction =
            ((self.track.bottom - y as f32) / (self.track.bottom - self.track.top)).clamp(0.0, 1.0);
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
                    14.0 * scale,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                )?,
                icon: text_format(
                    &factory,
                    w!("Segoe MDL2 Assets"),
                    16.0 * scale,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                )?,
                brush: unsafe { target.CreateSolidColorBrush(&INK, None)? },
            });
        }
        let painter = painter.as_ref().unwrap();
        let brush = &painter.brush;
        let fill = |rect: PxRect, radius: f32, color| unsafe {
            if rect.bottom > rect.top {
                brush.SetColor(&color);
                target.FillRoundedRectangle(&rect.as_rounded(radius * scale), brush);
            }
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
        let panel = PxRect::new(
            0.5,
            0.5,
            layout.size.cx as f32 - 0.5,
            layout.size.cy as f32 - 0.5,
        );
        fill(
            panel,
            15.0,
            D2D1_COLOR_F {
                a: 0.95,
                ..rgb(0x1b1410)
            },
        );
        unsafe {
            brush.SetColor(&TRACK);
            target.DrawRoundedRectangle(&panel.as_rounded(15.0 * scale), brush, 1.0, None);
        }
        let boosted = preferences.volume > 100;
        let amount_color = if preferences.muted {
            MUTED
        } else if boosted {
            HOT_ACCENT
        } else {
            INK
        };
        let volume: Vec<u16> = format!("{}%", preferences.volume).encode_utf16().collect();
        label(
            &volume,
            PxRect::new(0.0, 7.0 * scale, panel.right, 35.0 * scale),
            &painter.percent,
            amount_color,
        );
        let center = layout.size.cx as f32 * 0.5;
        let track = PxRect::new(
            center - 3.0 * scale,
            layout.track.top,
            center + 3.0 * scale,
            layout.track.bottom,
        );
        let knob = layout.track_y(preferences.volume);
        let nominal = layout.track_y(100);
        fill(track, 3.0, TRACK);
        let fill_color = if preferences.muted {
            rgb(0x877367)
        } else {
            ACCENT
        };
        fill(
            PxRect {
                top: knob.max(nominal),
                ..track
            },
            3.0,
            fill_color,
        );
        if boosted {
            fill(
                PxRect {
                    top: knob,
                    bottom: nominal,
                    ..track
                },
                2.0,
                if preferences.muted {
                    fill_color
                } else {
                    HOT_ACCENT
                },
            );
        }
        fill(
            PxRect::new(
                center + 7.0 * scale,
                nominal - scale,
                center + 13.0 * scale,
                nominal + scale,
            ),
            1.0,
            MUTED,
        );
        let half_width = if dragging || hover == Some(VolumeHit::Track) {
            8.0
        } else {
            6.0
        } * scale;
        fill(
            PxRect::new(
                center - half_width,
                knob - 2.5 * scale,
                center + half_width,
                knob + 2.5 * scale,
            ),
            2.5,
            if preferences.muted { MUTED } else { HOT_ACCENT },
        );
        if hover == Some(VolumeHit::Mute) {
            fill(layout.mute, 8.0, rgb(0x4b2d1e));
        }
        label(
            &[speaker_glyph(preferences)],
            layout.mute,
            &painter.icon,
            amount_color,
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
        assert_eq!(volume.value(-100), MAX_VOLUME);
        assert_eq!(volume.value(500), 0);
        assert!((volume.value(volume.track_y(100).round() as i32) - 100).abs() <= 1);
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
