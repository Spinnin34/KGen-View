use super::*;
use crate::imaging::{ResampleFilter, editable_size};

const MODE: i32 = 200;
const WIDTH_INPUT: i32 = 201;
const HEIGHT_INPUT: i32 = 202;
const LOCK: i32 = 203;
const FILTER: i32 = 204;
const MODE_ALT: i32 = 205;
const TRANSPARENT_FILL: i32 = 207;
const COLOR: i32 = 208;
const SUMMARY: i32 = 209;
const ERROR: i32 = 210;
const LEFT: i32 = 211;
const RIGHT: i32 = 212;
const TOP: i32 = 213;
const BOTTOM: i32 = 214;
const SIZE_LABELS: [i32; 2] = [215, 216];
const MARGIN_LABELS: [i32; 4] = [218, 219, 220, 221];
const COLOR_LABEL: i32 = 223;
const ANCHOR: i32 = 230;
const PRESET: i32 = 240;
const HALF: i32 = 246;
const DOUBLE: i32 = 247;
const PRESETS: [(&str, u32, u32); 6] = [
    ("640 × 480", 640, 480),
    ("1024 × 768", 1024, 768),
    ("1280 × 720 (HD)", 1280, 720),
    ("1920 × 1080", 1920, 1080),
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

    pub fn client_size(&self) -> (i32, i32) {
        match self.tool {
            SizeTool::Canvas => (588, 284),
            SizeTool::Resize => (536, 294),
        }
    }

    pub fn create(&self, hwnd: HWND, d: &Dialog) -> HWND {
        let (width, height) = self.client_size();
        let current = format!(
            "{}   {} × {} px",
            d.text("Tamaño actual", "Current size"),
            self.original.0,
            self.original.1
        );
        let focus = match self.tool {
            SizeTool::Canvas => self.create_canvas(hwnd, d, &current),
            SizeTool::Resize => self.create_resize(hwnd, d, &current),
        };
        d.label(hwnd, ERROR, "", [13, height - 56, width - 26, 19]);
        d.divider(hwnd, 8, height - 34, width - 16);
        d.button(
            hwnd,
            ACCEPT,
            d.text("Aplicar", "Apply"),
            BS_DEFPUSHBUTTON,
            [width - 190, height - 28, 86, 22],
        );
        d.button(
            hwnd,
            CANCEL,
            d.text("Cancelar", "Cancel"),
            BS_PUSHBUTTON,
            [width - 96, height - 28, 86, 22],
        );
        self.refresh(hwnd, d);
        unsafe {
            SendMessageW(focus, EM_SETSEL, None, Some(LPARAM(-1)));
        }
        focus
    }

    fn create_canvas(&self, hwnd: HWND, d: &Dialog, current: &str) -> HWND {
        d.label(hwnd, 0, current, [14, 10, 265, 19]);
        d.label(hwnd, SUMMARY, "", [318, 10, 260, 19]);
        d.group(hwnd, d.text("Método", "Method"), [8, 33, 301, 192]);
        d.group(hwnd, d.text("Anclaje", "Anchor"), [317, 33, 263, 110]);
        d.group(
            hwnd,
            d.text("Relleno del lienzo", "Canvas fill"),
            [317, 147, 263, 78],
        );

        self.mode(
            hwnd,
            d,
            MODE_ALT,
            d.text("Márgenes en píxeles", "Margins in pixels"),
            [18, 50, 230, 18],
        );
        for (index, (label, id)) in [
            (d.text("Izquierda", "Left"), LEFT),
            (d.text("Derecha", "Right"), RIGHT),
            (d.text("Arriba", "Top"), TOP),
            (d.text("Abajo", "Bottom"), BOTTOM),
        ]
        .into_iter()
        .enumerate()
        {
            let x = 24 + (index % 2) as i32 * 140;
            let y = 75 + (index / 2) as i32 * 25;
            d.label(hwnd, MARGIN_LABELS[index], label, [x, y + 2, 64, 18]);
            d.input(hwnd, id, "0", [x + 66, y, 57, 20]);
        }
        d.label(
            hwnd,
            0,
            d.text(
                "Los valores negativos recortan la imagen.",
                "Negative values crop the image.",
            ),
            [24, 127, 270, 18],
        );
        d.divider(hwnd, 18, 151, 281);
        self.mode(
            hwnd,
            d,
            MODE,
            d.text("Dimensiones finales", "Final dimensions"),
            [18, 161, 226, 18],
        );
        let width = self.dimension_fields(hwnd, d, 24, 186, 140);

        let anchors = [
            d.text("Sup. izq.", "Top left"),
            d.text("Sup. centro", "Top middle"),
            d.text("Sup. der.", "Top right"),
            d.text("Centro izq.", "Mid. left"),
            d.text("Centro", "Center"),
            d.text("Centro der.", "Mid. right"),
            d.text("Inf. izq.", "Bot. left"),
            d.text("Inf. centro", "Bot. middle"),
            d.text("Inf. der.", "Bot. right"),
        ];
        for (index, caption) in anchors.iter().enumerate() {
            let mut style = WINDOW_STYLE(BS_AUTORADIOBUTTON as u32);
            if index == 0 {
                style |= WS_GROUP;
            }
            if index == 4 {
                style |= WS_TABSTOP;
            }
            d.radio(
                hwnd,
                ANCHOR + index as i32,
                caption,
                style,
                [
                    327 + (index % 3) as i32 * 83,
                    53 + (index / 3) as i32 * 25,
                    83,
                    20,
                ],
            );
        }
        send(control(hwnd, ANCHOR + 4), BM_SETCHECK, 1);
        d.checkbox(
            hwnd,
            TRANSPARENT_FILL,
            d.text("Transparente", "Transparent"),
            true,
            [329, 165, 228, 20],
        );
        d.label(
            hwnd,
            COLOR_LABEL,
            d.text("Color hexadecimal", "Hex color"),
            [330, 193, 130, 18],
        );
        d.input(hwnd, COLOR, "#000000", [466, 190, 96, 21]);
        self.show_canvas_mode(hwnd);
        width
    }

    fn create_resize(&self, hwnd: HWND, d: &Dialog, current: &str) -> HWND {
        d.group(
            hwnd,
            d.text("Tamaño de imagen", "Image size"),
            [8, 5, 257, 64],
        );
        d.group(
            hwnd,
            d.text("Dimensiones nuevas", "New dimensions"),
            [8, 73, 257, 105],
        );
        d.group(
            hwnd,
            d.text("Filtro de remuestreo", "Resampling filter"),
            [8, 182, 257, 51],
        );
        d.group(
            hwnd,
            d.text("Tamaños predefinidos", "Standard sizes"),
            [273, 5, 255, 228],
        );
        d.label(hwnd, 0, current, [20, 24, 230, 18]);
        d.label(hwnd, SUMMARY, "", [20, 44, 230, 18]);

        self.mode(hwnd, d, MODE, d.text("Píxeles", "Pixels"), [20, 92, 80, 18]);
        self.mode(
            hwnd,
            d,
            MODE_ALT,
            d.text("Porcentaje", "Percent"),
            [108, 92, 135, 18],
        );
        let width = self.dimension_fields(hwnd, d, 20, 118, 121);
        d.checkbox(
            hwnd,
            LOCK,
            d.text("Mantener proporción", "Keep aspect ratio"),
            true,
            [20, 145, 224, 22],
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
            [20, 201, 229, 122],
        );

        for (index, &(label, _, _)) in PRESETS.iter().enumerate() {
            d.button(
                hwnd,
                PRESET + index as i32,
                label,
                BS_PUSHBUTTON,
                [
                    284 + (index % 2) as i32 * 119,
                    26 + (index / 2) as i32 * 26,
                    113,
                    22,
                ],
            );
        }
        d.divider(hwnd, 284, 108, 232);
        d.button(
            hwnd,
            HALF,
            d.text("Mitad", "Half"),
            BS_PUSHBUTTON,
            [284, 119, 113, 23],
        );
        d.button(
            hwnd,
            DOUBLE,
            d.text("Doble", "Double"),
            BS_PUSHBUTTON,
            [403, 119, 113, 23],
        );
        d.label(
            hwnd,
            0,
            d.text(
                "Con la proporción activa, la imagen se ajusta al tamaño elegido.",
                "With aspect ratio locked, the image fits within the selected size.",
            ),
            [284, 158, 228, 44],
        );
        width
    }

    fn mode(&self, hwnd: HWND, d: &Dialog, id: i32, title: &str, bounds: [i32; 4]) {
        let mode = d.radio(
            hwnd,
            id,
            title,
            WS_TABSTOP | WINDOW_STYLE(BS_RADIOBUTTON as u32),
            bounds,
        );
        send(mode, BM_SETCHECK, usize::from(id == MODE));
    }

    fn dimension_fields(&self, hwnd: HWND, d: &Dialog, x: i32, y: i32, spacing: i32) -> HWND {
        d.label(
            hwnd,
            SIZE_LABELS[0],
            d.text("Ancho", "Width"),
            [x, y + 2, 48, 18],
        );
        let width = d.input(
            hwnd,
            WIDTH_INPUT,
            &self.original.0.to_string(),
            [x + 49, y, 62, 20],
        );
        d.label(
            hwnd,
            SIZE_LABELS[1],
            d.text("Alto", "Height"),
            [x + spacing, y + 2, 43, 18],
        );
        d.input(
            hwnd,
            HEIGHT_INPUT,
            &self.original.1.to_string(),
            [x + spacing + 44, y, 62, 20],
        );
        width
    }

    fn show_canvas_mode(&self, hwnd: HWND) {
        let margins = checked(hwnd, MODE_ALT);
        for id in [WIDTH_INPUT, HEIGHT_INPUT]
            .into_iter()
            .chain(SIZE_LABELS)
            .chain(ANCHOR..ANCHOR + 9)
        {
            unsafe {
                let _ = EnableWindow(control(hwnd, id), !margins);
            }
        }
        for id in [LEFT, RIGHT, TOP, BOTTOM].into_iter().chain(MARGIN_LABELS) {
            unsafe {
                let _ = EnableWindow(control(hwnd, id), margins);
            }
        }
    }

    pub fn command(&mut self, hwnd: HWND, d: &Dialog, id: i32, notification: u32) -> bool {
        if id == ACCEPT {
            self.result = self.operation(hwnd).ok().flatten();
            return self.result.is_some();
        }
        let method_changed = matches!(id, MODE | MODE_ALT) && notification == BN_CLICKED;
        if method_changed {
            send(control(hwnd, MODE), BM_SETCHECK, usize::from(id == MODE));
            send(
                control(hwnd, MODE_ALT),
                BM_SETCHECK,
                usize::from(id == MODE_ALT),
            );
        }
        match self.tool {
            SizeTool::Canvas if method_changed => self.show_canvas_mode(hwnd),
            SizeTool::Resize if method_changed && self.percent != (id == MODE_ALT) => {
                let size = self.resize_dimensions(hwnd).unwrap_or(self.original);
                self.percent = id == MODE_ALT;
                self.set_size(hwnd, size);
            }
            SizeTool::Resize if (PRESET..=DOUBLE).contains(&id) && notification == BN_CLICKED => {
                self.apply_preset(hwnd, id);
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
        let result = if checked(hwnd, MODE_ALT) {
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
            let anchor = (0..9).find(|i| checked(hwnd, ANCHOR + i)).unwrap_or(4);
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

    fn apply_preset(&mut self, hwnd: HWND, id: i32) {
        let (w, h) = self.original;
        let size = match id {
            HALF => (w.div_ceil(2), h.div_ceil(2)),
            DOUBLE => (w * 2, h * 2),
            _ => {
                let (_, width, height) = PRESETS[(id - PRESET) as usize];
                (width, height)
            }
        };
        self.set_size(hwnd, size);
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
