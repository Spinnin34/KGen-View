use super::*;
use crate::imaging::{ResampleFilter, editable_size};

pub(super) const HEIGHT: i32 = 332;
const MODE: i32 = 200;
const WIDTH_INPUT: i32 = 201;
const HEIGHT_INPUT: i32 = 202;
const LOCK: i32 = 203;
const FILTER: i32 = 204;
const PRESET: i32 = 205;
const ANCHOR: i32 = 206;
const TRANSPARENT_FILL: i32 = 207;
const COLOR: i32 = 208;
const SUMMARY: i32 = 209;
const ERROR: i32 = 210;
const LEFT: i32 = 211;
const RIGHT: i32 = 212;
const TOP: i32 = 213;
const BOTTOM: i32 = 214;
const SIZE_LABELS: [i32; 3] = [215, 216, 217];
const MARGIN_LABELS: [i32; 4] = [218, 219, 220, 221];
const ANCHOR_LABEL: i32 = 222;
const COLOR_LABEL: i32 = 223;
const PRESETS: [(&str, u32, u32); 6] = [
    ("640 × 480", 640, 480),
    ("1024 × 768", 1024, 768),
    ("1280 × 720 (HD)", 1280, 720),
    ("1920 × 1080 (Full HD)", 1920, 1080),
    ("2560 × 1440 (QHD)", 2560, 1440),
    ("3840 × 2160 (4K)", 3840, 2160),
];

#[derive(Clone, Copy)]
pub enum SizeTool {
    Canvas,
    Resize,
}

pub(super) struct Panel {
    tool: SizeTool,
    original: (u32, u32),
    percent: bool,
    last_axis: i32,
    pub result: Option<EditOperation>,
}

impl Panel {
    pub fn new(tool: SizeTool, original: (u32, u32)) -> Self {
        Self {
            tool,
            original,
            percent: false,
            last_axis: WIDTH_INPUT,
            result: None,
        }
    }

    pub fn create(&mut self, hwnd: HWND, d: &Dialog) -> HWND {
        d.label(
            hwnd,
            0,
            &format!(
                "{}   {} × {} px",
                d.text("Tamaño actual", "Current size"),
                self.original.0,
                self.original.1
            ),
            [22, 19, 244, 23],
        );
        d.label(hwnd, SUMMARY, "", [274, 19, 240, 23]);
        let (mode_label, mode_items) = match self.tool {
            SizeTool::Canvas => (
                d.text("Método", "Method"),
                [
                    d.text("Dimensiones finales", "Final dimensions"),
                    d.text("Márgenes por lado", "Margins per side"),
                ],
            ),
            SizeTool::Resize => (
                d.text("Unidades", "Units"),
                [d.text("Píxeles", "Pixels"), d.text("Porcentaje", "Percent")],
            ),
        };
        d.label(hwnd, 0, mode_label, [22, 63, 112, 22]);
        d.combo(hwnd, MODE, &mode_items, 0, [140, 59, 202, 120]);
        d.label(
            hwnd,
            SIZE_LABELS[0],
            d.text("Ancho", "Width"),
            [22, 106, 105, 22],
        );
        let width = d.input(
            hwnd,
            WIDTH_INPUT,
            &self.original.0.to_string(),
            [140, 102, 106, 25],
        );
        d.label(
            hwnd,
            SIZE_LABELS[1],
            d.text("Alto", "Height"),
            [272, 106, 110, 22],
        );
        d.input(
            hwnd,
            HEIGHT_INPUT,
            &self.original.1.to_string(),
            [390, 102, 106, 25],
        );
        match self.tool {
            SizeTool::Canvas => self.create_canvas(hwnd, d),
            SizeTool::Resize => self.create_resize(hwnd, d),
        }
        d.label(hwnd, ERROR, "", [22, 260, 490, 22]);
        d.button(
            hwnd,
            ACCEPT,
            d.text("Aplicar", "Apply"),
            BS_DEFPUSHBUTTON,
            [304, 287, 102, 29],
        );
        d.button(
            hwnd,
            CANCEL,
            d.text("Cancelar", "Cancel"),
            BS_PUSHBUTTON,
            [412, 287, 102, 29],
        );
        self.refresh(hwnd, d);
        unsafe {
            SendMessageW(width, EM_SETSEL, None, Some(LPARAM(-1)));
        }
        width
    }

    fn create_resize(&self, hwnd: HWND, d: &Dialog) {
        d.checkbox(
            hwnd,
            LOCK,
            d.text("Mantener proporción", "Keep aspect ratio"),
            true,
            [140, 139, 230, 24],
        );
        d.label(hwnd, 0, d.text("Preajuste", "Preset"), [22, 184, 112, 22]);
        let mut items = vec![
            d.text("Elegir un tamaño", "Choose a size"),
            d.text("Mitad", "Half"),
            d.text("Doble", "Double"),
        ];
        items.extend(PRESETS.iter().map(|preset| preset.0));
        d.combo(hwnd, PRESET, &items, 0, [140, 180, 270, 240]);
        d.label(
            hwnd,
            0,
            d.text("Remuestreo", "Resampling"),
            [22, 223, 112, 22],
        );
        d.combo(
            hwnd,
            FILTER,
            &[
                d.text("Bicúbico de alta calidad", "High-quality bicubic"),
                d.text("Promedio de área", "Area averaging"),
                d.text("Bilineal", "Bilinear"),
                d.text("Vecino más cercano", "Nearest neighbor"),
            ],
            0,
            [140, 219, 270, 144],
        );
    }

    fn create_canvas(&self, hwnd: HWND, d: &Dialog) {
        for (i, (label, id)) in [
            (d.text("Izquierda", "Left"), LEFT),
            (d.text("Derecha", "Right"), RIGHT),
            (d.text("Arriba", "Top"), TOP),
            (d.text("Abajo", "Bottom"), BOTTOM),
        ]
        .into_iter()
        .enumerate()
        {
            let x = if i % 2 == 0 { 22 } else { 272 };
            let y = 102 + (i / 2) as i32 * 37;
            d.label(hwnd, MARGIN_LABELS[i], label, [x, y + 4, 112, 22]);
            d.input(hwnd, id, "0", [x + 118, y, 106, 25]);
        }
        d.label(
            hwnd,
            SIZE_LABELS[2],
            d.text("Posición de la imagen", "Image position"),
            [22, 151, 143, 22],
        );
        d.combo(
            hwnd,
            ANCHOR,
            &[
                d.text("Superior izquierda", "Top left"),
                d.text("Superior centro", "Top center"),
                d.text("Superior derecha", "Top right"),
                d.text("Centro izquierda", "Center left"),
                d.text("Centro", "Center"),
                d.text("Centro derecha", "Center right"),
                d.text("Inferior izquierda", "Bottom left"),
                d.text("Inferior centro", "Bottom center"),
                d.text("Inferior derecha", "Bottom right"),
            ],
            4,
            [172, 147, 206, 224],
        );
        d.label(
            hwnd,
            ANCHOR_LABEL,
            d.text(
                "Los márgenes negativos recortan la imagen.",
                "Negative margins crop the image.",
            ),
            [22, 176, 474, 22],
        );
        d.checkbox(
            hwnd,
            TRANSPARENT_FILL,
            d.text("Relleno transparente", "Transparent fill"),
            true,
            [22, 218, 210, 25],
        );
        d.label(
            hwnd,
            COLOR_LABEL,
            d.text("Color hexadecimal", "Hex color"),
            [247, 220, 133, 22],
        );
        d.input(hwnd, COLOR, "#000000", [390, 216, 106, 25]);
        self.show_canvas_mode(hwnd);
    }

    fn show_canvas_mode(&self, hwnd: HWND) {
        let margins = selected(hwnd, MODE) == 1;
        for id in [WIDTH_INPUT, HEIGHT_INPUT, ANCHOR]
            .into_iter()
            .chain(SIZE_LABELS)
        {
            unsafe {
                let _ = ShowWindow(control(hwnd, id), if margins { SW_HIDE } else { SW_SHOW });
            }
        }
        for id in [LEFT, RIGHT, TOP, BOTTOM, ANCHOR_LABEL]
            .into_iter()
            .chain(MARGIN_LABELS)
        {
            unsafe {
                let _ = ShowWindow(control(hwnd, id), if margins { SW_SHOW } else { SW_HIDE });
            }
        }
    }

    pub fn command(&mut self, hwnd: HWND, d: &Dialog, id: i32, notification: u32) -> bool {
        if id == ACCEPT {
            self.result = self.operation(hwnd).ok().flatten();
            return self.result.is_some();
        }
        match self.tool {
            SizeTool::Canvas if id == MODE && notification == CBN_SELCHANGE => {
                self.show_canvas_mode(hwnd)
            }
            SizeTool::Resize if id == MODE && notification == CBN_SELCHANGE => {
                let size = self.resize_dimensions(hwnd).unwrap_or(self.original);
                self.percent = selected(hwnd, MODE) == 1;
                self.set_size(hwnd, size);
            }
            SizeTool::Resize if id == PRESET && notification == CBN_SELCHANGE => {
                self.apply_preset(hwnd);
            }
            SizeTool::Resize
                if id == LOCK
                    || (matches!(id, WIDTH_INPUT | HEIGHT_INPUT) && notification == EN_CHANGE) =>
            {
                if id != LOCK {
                    self.last_axis = id;
                }
                self.sync_axes(hwnd);
            }
            _ => {}
        }
        self.refresh(hwnd, d);
        false
    }

    fn refresh(&self, hwnd: HWND, d: &Dialog) {
        if matches!(self.tool, SizeTool::Canvas) {
            let has_color = !checked(hwnd, TRANSPARENT_FILL);
            unsafe {
                let _ = EnableWindow(control(hwnd, COLOR), has_color);
                let _ = EnableWindow(control(hwnd, COLOR_LABEL), has_color);
            }
        }
        let dimensions = match self.tool {
            SizeTool::Canvas => self.canvas_dimensions(hwnd).map(|(size, _)| size),
            SizeTool::Resize => self.resize_dimensions(hwnd),
        };
        let size = dimensions.map_or_else(|| "—".into(), |(w, h)| format!("{w} × {h} px"));
        set_text(
            control(hwnd, SUMMARY),
            &format!("{}   {size}", d.text("Nuevo tamaño", "New size")),
        );
        let (message, apply) = match self.operation(hwnd) {
            Ok(Some(_)) => ("", true),
            Ok(None) => ("", false),
            Err(Invalid::Size) => (
                d.text(
                    "Introduce dimensiones válidas dentro del límite de memoria.",
                    "Enter valid dimensions within the memory limit.",
                ),
                false,
            ),
            Err(Invalid::Color) => (
                d.text(
                    "Introduce un color de seis dígitos, por ejemplo #2458A0.",
                    "Enter a six-digit color, for example #2458A0.",
                ),
                false,
            ),
        };
        set_text(control(hwnd, ERROR), message);
        unsafe {
            let _ = EnableWindow(control(hwnd, ACCEPT), apply);
        }
    }

    fn operation(&self, hwnd: HWND) -> Result<Option<EditOperation>, Invalid> {
        let op = match self.tool {
            SizeTool::Canvas => {
                let (size, offset) = self.canvas_dimensions(hwnd).ok_or(Invalid::Size)?;
                if size == self.original && offset == (0, 0) {
                    return Ok(None);
                }
                let fill = if checked(hwnd, TRANSPARENT_FILL) {
                    [0; 4]
                } else {
                    let text = value(hwnd, COLOR);
                    let hex = text.trim().trim_start_matches('#');
                    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(Invalid::Color);
                    }
                    let rgb = u32::from_str_radix(hex, 16).map_err(|_| Invalid::Color)?;
                    [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 255]
                };
                EditOperation::Canvas { size, offset, fill }
            }
            SizeTool::Resize => {
                let size = self.resize_dimensions(hwnd).ok_or(Invalid::Size)?;
                if size == self.original {
                    return Ok(None);
                }
                let filter = [
                    ResampleFilter::Cubic,
                    ResampleFilter::Area,
                    ResampleFilter::Linear,
                    ResampleFilter::Nearest,
                ][selected(hwnd, FILTER)];
                EditOperation::Resize { size, filter }
            }
        };
        Ok(Some(op))
    }

    fn canvas_dimensions(&self, hwnd: HWND) -> Option<((u32, u32), (i32, i32))> {
        let (width, height) = self.original;
        let result = if selected(hwnd, MODE) == 1 {
            let read = |id| value(hwnd, id).trim().parse::<i32>().ok().map(i64::from);
            let (left, right, top, bottom) = (read(LEFT)?, read(RIGHT)?, read(TOP)?, read(BOTTOM)?);
            let size = (
                u32::try_from(i64::from(width) + left + right).ok()?,
                u32::try_from(i64::from(height) + top + bottom).ok()?,
            );
            (size, (left as i32, top as i32))
        } else {
            let size = (
                read_pixels(hwnd, WIDTH_INPUT)?,
                read_pixels(hwnd, HEIGHT_INPUT)?,
            );
            if !editable_size(size) {
                return None;
            }
            let anchor = selected(hwnd, ANCHOR);
            let shift = |delta: i32, align| match align {
                0 => 0,
                1 => delta.div_euclid(2),
                _ => delta,
            };
            let offset = (
                shift(size.0 as i32 - width as i32, anchor % 3),
                shift(size.1 as i32 - height as i32, anchor / 3),
            );
            (size, offset)
        };
        editable_size(result.0).then_some(result)
    }

    fn resize_dimensions(&self, hwnd: HWND) -> Option<(u32, u32)> {
        let (w, h) = self.original;
        let locked = checked(hwnd, LOCK);
        let size = if self.percent {
            let width = read_percent(hwnd, if locked { self.last_axis } else { WIDTH_INPUT })?;
            let height = if locked {
                width
            } else {
                read_percent(hwnd, HEIGHT_INPUT)?
            };
            (
                rounded(f64::from(w) * width / 100.0)?,
                rounded(f64::from(h) * height / 100.0)?,
            )
        } else if locked && self.last_axis == HEIGHT_INPUT {
            let height = read_pixels(hwnd, HEIGHT_INPUT)?;
            (
                rounded(f64::from(height) * f64::from(w) / f64::from(h))?,
                height,
            )
        } else if locked {
            let width = read_pixels(hwnd, WIDTH_INPUT)?;
            (
                width,
                rounded(f64::from(width) * f64::from(h) / f64::from(w))?,
            )
        } else {
            (
                read_pixels(hwnd, WIDTH_INPUT)?,
                read_pixels(hwnd, HEIGHT_INPUT)?,
            )
        };
        editable_size(size).then_some(size)
    }

    fn sync_axes(&self, hwnd: HWND) {
        if !checked(hwnd, LOCK) {
            return;
        }
        let width_changed = self.last_axis == WIDTH_INPUT;
        let target = if width_changed {
            HEIGHT_INPUT
        } else {
            WIDTH_INPUT
        };
        let adjusted = if self.percent {
            read_percent(hwnd, self.last_axis).map(|_| value(hwnd, self.last_axis))
        } else {
            self.resize_dimensions(hwnd)
                .map(|size| if width_changed { size.1 } else { size.0 })
                .map(|n| n.to_string())
        };
        if let Some(adjusted) = adjusted {
            set_text(control(hwnd, target), &adjusted);
        }
    }

    fn set_size(&mut self, hwnd: HWND, (width, height): (u32, u32)) {
        let (w, h) = self.original;
        self.last_axis = if checked(hwnd, LOCK)
            && u64::from(width) * u64::from(h) > u64::from(height) * u64::from(w)
        {
            HEIGHT_INPUT
        } else {
            WIDTH_INPUT
        };
        if self.percent {
            let width_percent = f64::from(width) * 100.0 / f64::from(w);
            let height_percent = f64::from(height) * 100.0 / f64::from(h);
            let (wp, hp) = if checked(hwnd, LOCK) {
                let percent = width_percent.min(height_percent);
                (percent, percent)
            } else {
                (width_percent, height_percent)
            };
            set_text(control(hwnd, WIDTH_INPUT), &format_percent(wp));
            set_text(control(hwnd, HEIGHT_INPUT), &format_percent(hp));
        } else {
            set_text(control(hwnd, WIDTH_INPUT), &width.to_string());
            set_text(control(hwnd, HEIGHT_INPUT), &height.to_string());
            self.sync_axes(hwnd);
        }
    }

    fn apply_preset(&mut self, hwnd: HWND) {
        let (w, h) = self.original;
        let size = match selected(hwnd, PRESET) {
            0 => return,
            1 => (w.div_ceil(2), h.div_ceil(2)),
            2 => (w * 2, h * 2),
            n => {
                let (_, width, height) = PRESETS[n - 3];
                (width, height)
            }
        };
        self.set_size(hwnd, size);
        send(control(hwnd, PRESET), CB_SETCURSEL, 0);
    }
}

enum Invalid {
    Size,
    Color,
}

fn read_pixels(hwnd: HWND, id: i32) -> Option<u32> {
    value(hwnd, id).trim().parse().ok()
}

fn read_percent(hwnd: HWND, id: i32) -> Option<f64> {
    let number: f64 = value(hwnd, id).trim().replace(',', ".").parse().ok()?;
    (number.is_finite() && number > 0.0).then_some(number)
}

fn format_percent(percent: f64) -> String {
    format!("{percent:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn rounded(value: f64) -> Option<u32> {
    (value > 0.0 && value.round() <= 100_000.0).then(|| value.round().max(1.0) as u32)
}
