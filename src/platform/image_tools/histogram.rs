use super::*;
use windows::Win32::{
    Foundation::POINT,
    UI::Controls::{
        ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TBM_SETPOS, TBM_SETRANGEMAX,
        TBS_NOTICKS, TRACKBAR_CLASSW,
    },
};

pub(super) const HEIGHT: i32 = 430;
const CHANNELS: [i32; 4] = [300, 301, 302, 303];
const LOG_SCALE: i32 = 304;
const SLIDER: i32 = 305;
const TBM_GETPOS: u32 = WM_USER;
const BIN: i32 = 306;
const COUNTS: i32 = 310;
const SHARES: i32 = 320;

pub(super) struct Panel {
    data: Box<Histogram>,
    enabled: [bool; 4],
    logarithmic: bool,
    bin: usize,
}

impl Panel {
    pub fn new(data: Box<Histogram>) -> Self {
        Self {
            data,
            enabled: [true; 4],
            logarithmic: false,
            bin: 128,
        }
    }

    pub fn create(&mut self, hwnd: HWND, d: &Dialog) -> HWND {
        let channels = [
            d.text("Rojo", "Red"),
            d.text("Verde", "Green"),
            d.text("Azul", "Blue"),
            d.text("Luminancia", "Luminance"),
        ];
        for (i, caption) in channels.iter().enumerate() {
            d.checkbox(
                hwnd,
                CHANNELS[i],
                caption,
                true,
                [22 + i as i32 * 80, 12, if i == 3 { 111 } else { 76 }, 24],
            );
        }
        d.checkbox(
            hwnd,
            LOG_SCALE,
            d.text("Escala logarítmica", "Log scale"),
            false,
            [378, 12, 145, 24],
        );
        for (x, label) in [
            (22, "0"),
            (141, "64"),
            (259, "128"),
            (382, "192"),
            (488, "255"),
        ] {
            d.label(hwnd, 0, label, [x, 211, 30, 20]);
        }
        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_BAR_CLASSES,
            });
        }
        let slider = d.control(
            hwnd,
            SLIDER,
            TRACKBAR_CLASSW,
            "",
            WS_TABSTOP | WINDOW_STYLE(TBS_NOTICKS),
            [16, 229, 504, 25],
        );
        unsafe {
            SendMessageW(slider, TBM_SETRANGEMAX, Some(WPARAM(1)), Some(LPARAM(255)));
        }
        set_slider(slider, self.bin);
        d.label(hwnd, BIN, "", [22, 265, 196, 22]);
        d.label(
            hwnd,
            0,
            &format!(
                "{}  {} / {}",
                d.text("Píxeles visibles / total", "Visible / total pixels"),
                count(d, self.data.visible),
                count(d, self.data.pixels)
            ),
            [232, 265, 284, 22],
        );
        d.label(
            hwnd,
            0,
            d.text("Recuento", "Pixel count"),
            [22, 316, 104, 20],
        );
        d.label(
            hwnd,
            0,
            d.text("Porcentaje", "Percentage"),
            [22, 340, 104, 20],
        );
        d.label(hwnd, 0, d.text("Media", "Mean"), [22, 364, 104, 20]);
        for (i, caption) in channels.iter().enumerate() {
            let x = 133 + i as i32 * 96;
            d.label(hwnd, 0, caption, [x, 294, 95, 20]);
            d.label(hwnd, COUNTS + i as i32, "", [x, 316, 96, 20]);
            d.label(hwnd, SHARES + i as i32, "", [x, 340, 96, 20]);
            let mean = if self.data.visible == 0 {
                "—".into()
            } else {
                let sum: u64 = self.data.channels[i]
                    .iter()
                    .enumerate()
                    .map(|(bin, &n)| bin as u64 * n)
                    .sum();
                decimal(d, sum as f64 / self.data.visible as f64)
            };
            d.label(hwnd, 0, &mean, [x, 364, 95, 20]);
        }
        let close = d.button(
            hwnd,
            ACCEPT,
            d.text("Cerrar", "Close"),
            BS_DEFPUSHBUTTON,
            [412, 389, 102, 29],
        );
        self.update_stats(hwnd, d);
        close
    }

    pub fn command(&mut self, hwnd: HWND, dialog: &Dialog, id: i32) -> bool {
        if id == ACCEPT {
            return true;
        }
        if let Some(channel) = CHANNELS.iter().position(|&channel| channel == id) {
            self.enabled[channel] = checked(hwnd, id);
        } else if id == LOG_SCALE {
            self.logarithmic = checked(hwnd, id);
        }
        self.redraw(hwnd, dialog);
        false
    }

    pub fn interact(&mut self, hwnd: HWND, d: &Dialog, message: u32, lparam: LPARAM) {
        let slider = control(hwnd, SLIDER);
        let index = if message == WM_HSCROLL && lparam.0 == slider.0 as isize {
            send(slider, TBM_GETPOS, 0).clamp(0, 255) as usize
        } else if message == WM_MOUSEMOVE {
            let rect = plot(d);
            let x = lparam.0 as u16 as i16 as i32;
            let y = (lparam.0 >> 16) as u16 as i16 as i32;
            if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom {
                return;
            }
            let width = (rect.right - rect.left - 1).max(1);
            let position = ((x - rect.left) * 255 + width / 2) / width;
            set_slider(slider, position as usize);
            position as usize
        } else {
            return;
        };
        if index != self.bin {
            self.bin = index;
            self.update_stats(hwnd, d);
            self.redraw(hwnd, d);
        }
    }

    fn update_stats(&self, hwnd: HWND, d: &Dialog) {
        set_text(
            control(hwnd, BIN),
            &format!("{}   {}", d.text("Nivel", "Level"), self.bin),
        );
        for (i, channel) in self.data.channels.iter().enumerate() {
            let n = channel[self.bin];
            let percent = 100.0 * n as f64 / self.data.visible.max(1) as f64;
            set_text(control(hwnd, COUNTS + i as i32), &count(d, n));
            set_text(
                control(hwnd, SHARES + i as i32),
                &format!("{}%", decimal(d, percent)),
            );
        }
    }

    fn redraw(&self, hwnd: HWND, d: &Dialog) {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&plot(d)), false);
        }
    }

    pub fn paint(&self, dc: HDC, d: &Dialog) {
        let rect = plot(d);
        let palette = if d.dark {
            [
                0x006b6bff, 0x007bdb66, 0x00ffa072, 0x00c1c1c1, 0x00151413, 0x0046423e, 0x008a8783,
            ]
        } else {
            [
                0x003a35cc, 0x004b8017, 0x00c14623, 0x00625d59, 0x00ffffff, 0x00e7e3e0, 0x00a29d99,
            ]
        };
        let x = |bin: usize| rect.left + (rect.right - rect.left - 1) * bin as i32 / 255;
        let bottom = rect.bottom - 1;
        let brush = unsafe { CreateSolidBrush(COLORREF(palette[4])) };
        unsafe {
            FillRect(dc, &rect, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            let clip = SaveDC(dc);
            if clip == 0 {
                return;
            }
            IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
            for bin in [0, 64, 128, 192, 255] {
                line(
                    dc,
                    palette[5],
                    &[
                        POINT {
                            x: x(bin),
                            y: rect.top,
                        },
                        POINT {
                            x: x(bin),
                            y: rect.bottom,
                        },
                    ],
                );
            }
            for division in 0..=4 {
                let y = bottom - (bottom - rect.top) * division / 4;
                line(
                    dc,
                    palette[5],
                    &[POINT { x: rect.left, y }, POINT { x: rect.right, y }],
                );
            }
            line(
                dc,
                palette[6],
                &[
                    POINT {
                        x: x(self.bin),
                        y: rect.top,
                    },
                    POINT {
                        x: x(self.bin),
                        y: rect.bottom,
                    },
                ],
            );
            let max = self
                .data
                .channels
                .iter()
                .zip(self.enabled)
                .filter(|(_, enabled)| *enabled)
                .flat_map(|(channel, _)| channel)
                .copied()
                .max()
                .unwrap_or(0)
                .max(1);
            let scale = |n: u64| {
                if self.logarithmic {
                    (n as f64).ln_1p()
                } else {
                    n as f64
                }
            };
            let ratio = f64::from(bottom - rect.top - d.px(7)) / scale(max);
            for i in [3, 0, 1, 2] {
                if self.enabled[i] {
                    let points: [POINT; 256] = std::array::from_fn(|bin| POINT {
                        x: x(bin),
                        y: bottom - (scale(self.data.channels[i][bin]) * ratio).round() as i32,
                    });
                    line(dc, palette[i], &points);
                }
            }
            let _ = RestoreDC(dc, clip);
        }
    }
}

fn plot(d: &Dialog) -> RECT {
    d.rect(22, 47, 492, 160)
}

fn count(d: &Dialog, value: u64) -> String {
    let separator = d.text(".", ",");
    let digits = value.to_string();
    let mut output = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i != 0 && (digits.len() - i).is_multiple_of(3) {
            output.push_str(separator);
        }
        output.push(digit);
    }
    output
}

fn decimal(d: &Dialog, value: f64) -> String {
    let decimal = format!("{value:.2}");
    if d.language == Language::Spanish {
        decimal.replace('.', ",")
    } else {
        decimal
    }
}

fn set_slider(slider: HWND, position: usize) {
    unsafe {
        SendMessageW(
            slider,
            TBM_SETPOS,
            Some(WPARAM(1)),
            Some(LPARAM(position as isize)),
        );
    }
}

unsafe fn line(dc: HDC, color: u32, points: &[POINT]) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(color));
        if !pen.0.is_null() {
            let old = SelectObject(dc, HGDIOBJ(pen.0));
            let _ = Polyline(dc, points);
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }
}
