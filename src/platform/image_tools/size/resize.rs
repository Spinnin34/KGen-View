use super::*;
use crate::imaging::{ResampleFilter, ResizeOptions};

pub(in crate::platform::image_tools) const RESIZE_CLIENT: (i32, i32) = (638, 419);
const REGISTRY: &str = "ResizeStandardDimensions";
const SUMMARY: i32 = 200;
const MODE: i32 = 201;
const WIDTH: i32 = 202;
const HEIGHT: i32 = 203;
const MP: i32 = 204;
const UNITS: i32 = 205;
const LOCK: i32 = 206;
const DPI: i32 = 207;
const ADJUST_DPI: i32 = 208;
const SHARPEN: i32 = 209;
const QUALITY: i32 = 210;
const FAST: i32 = 211;
const FILTER: i32 = 212;
const FAST_REDUCE: i32 = 213;
const GAMMA: i32 = 214;
const STANDARD: i32 = 215;
const FIT_DESKTOP: i32 = 216;
const EXACT_DESKTOP: i32 = 217;
const HALF: i32 = 218;
const DOUBLE: i32 = 219;
const SWAP: i32 = 220;
const SAVE_PRESET: i32 = 221;
const STATUS: i32 = 222;
const STANDARD_SIZES: [(&str, (u32, u32)); 8] = [
    ("640 × 480", (640, 480)),
    ("800 × 600", (800, 600)),
    ("1024 × 768", (1024, 768)),
    ("1280 × 720 (HD)", (1280, 720)),
    ("1920 × 1080 (Full HD)", (1920, 1080)),
    ("2560 × 1440 (QHD)", (2560, 1440)),
    ("3840 × 2160 (4K)", (3840, 2160)),
    ("7680 × 4320 (8K)", (7680, 4320)),
];
const FILTERS: [ResampleFilter; 5] = [
    ResampleFilter::Lanczos,
    ResampleFilter::Cubic,
    ResampleFilter::Linear,
    ResampleFilter::Gaussian,
    ResampleFilter::Area,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Dimensions,
    Percent,
    Megapixels,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Pixels,
    Centimeters,
    Inches,
}

impl Unit {
    fn is_physical(self) -> bool {
        self != Self::Pixels
    }

    fn pixels_per_unit(self, dpi: f64) -> f64 {
        match self {
            Self::Pixels => 1.0,
            Self::Centimeters => dpi / 2.54,
            Self::Inches => dpi,
        }
    }
}

pub(in crate::platform::image_tools) struct ResizePanel {
    original: (u32, u32),
    original_dpi: (f64, f64),
    desktop: (u32, u32),
    last_axis: i32,
    mode: Mode,
    unit: Unit,
    manual_dpi: String,
    custom: Vec<(u32, u32)>,
    pub result: Option<EditOperation>,
}

impl ResizePanel {
    pub fn new(original: (u32, u32), dpi: (f64, f64), desktop: (u32, u32)) -> Self {
        Self {
            original,
            original_dpi: dpi,
            desktop,
            last_axis: WIDTH,
            mode: Mode::Dimensions,
            unit: Unit::Pixels,
            manual_dpi: decimal(dpi.0),
            custom: read_presets(REGISTRY),
            result: None,
        }
    }

    pub fn create(&self, hwnd: HWND, d: &Dialog) -> HWND {
        d.group(
            hwnd,
            d.text("Tamaño de imagen", "Image size"),
            [8, 5, 303, 66],
        );
        d.group(hwnd, d.text("Nuevo tamaño", "New size"), [8, 75, 303, 162]);
        d.group(
            hwnd,
            d.text("Proporción y resolución", "Aspect ratio and resolution"),
            [8, 241, 303, 118],
        );
        d.group(
            hwnd,
            d.text("Dimensiones estándar", "Standard dimensions"),
            [320, 5, 310, 174],
        );
        d.group(
            hwnd,
            d.text("Método de redimensionado", "Resize method"),
            [320, 183, 310, 176],
        );
        d.label(
            hwnd,
            0,
            &format!(
                "{}  {} px",
                d.text("Actual", "Current"),
                dimensions(self.original)
            ),
            [20, 26, 280, 18],
        );
        d.label(hwnd, SUMMARY, "", [20, 47, 280, 18]);
        d.combo(
            hwnd,
            MODE,
            &[
                d.text("Introducir ancho y alto", "Enter width and height"),
                d.text("Porcentaje del original", "Percent of original"),
                d.text("Megapíxeles", "Megapixels"),
            ],
            0,
            [20, 96, 276, 114],
        );
        d.label(hwnd, 0, d.text("Ancho", "Width"), [20, 129, 42, 18]);
        let width = d.input(hwnd, WIDTH, &self.original.0.to_string(), [67, 126, 69, 20]);
        d.label(hwnd, 0, d.text("Alto", "Height"), [150, 129, 45, 18]);
        d.input(
            hwnd,
            HEIGHT,
            &self.original.1.to_string(),
            [201, 126, 95, 20],
        );
        d.label(hwnd, 0, d.text("Unidades", "Units"), [20, 160, 82, 18]);
        d.combo(
            hwnd,
            UNITS,
            &[
                d.text("Píxeles", "Pixels"),
                "cm",
                d.text("Pulgadas", "Inches"),
            ],
            0,
            [109, 156, 187, 110],
        );
        d.label(hwnd, 0, "MP", [20, 193, 82, 18]);
        d.input(
            hwnd,
            MP,
            &decimal(f64::from(self.original.0) * f64::from(self.original.1) / 1e6),
            [109, 190, 98, 20],
        );

        d.checkbox(
            hwnd,
            LOCK,
            d.text("Mantener relación de aspecto", "Keep aspect ratio"),
            true,
            [20, 263, 276, 20],
        );
        d.checkbox(
            hwnd,
            ADJUST_DPI,
            d.text(
                "Ajustar DPI al nuevo tamaño",
                "Adjust DPI to match the new size",
            ),
            false,
            [20, 289, 276, 20],
        );
        d.label(hwnd, 0, "DPI", [22, 322, 80, 18]);
        d.input(hwnd, DPI, &self.manual_dpi, [109, 319, 95, 20]);

        d.combo(hwnd, STANDARD, &[], 0, [333, 26, 283, 238]);
        self.update_presets(hwnd, d);
        d.button(
            hwnd,
            FIT_DESKTOP,
            d.text("Ajustar al escritorio", "Fit to desktop"),
            BS_PUSHBUTTON,
            [333, 57, 139, 23],
        );
        d.button(
            hwnd,
            EXACT_DESKTOP,
            d.text("Escritorio exacto", "Exact desktop size"),
            BS_PUSHBUTTON,
            [478, 57, 138, 23],
        );
        d.button(
            hwnd,
            HALF,
            d.text("Mitad", "Half"),
            BS_PUSHBUTTON,
            [333, 89, 139, 23],
        );
        d.button(
            hwnd,
            DOUBLE,
            d.text("Doble", "Double"),
            BS_PUSHBUTTON,
            [478, 89, 138, 23],
        );
        d.button(
            hwnd,
            SWAP,
            d.text("Intercambiar lados", "Swap sides"),
            BS_PUSHBUTTON,
            [333, 121, 139, 23],
        );
        d.button(
            hwnd,
            SAVE_PRESET,
            d.text("Añadir a estándar", "Add to standard sizes"),
            BS_PUSHBUTTON,
            [478, 121, 138, 23],
        );
        d.label(
            hwnd,
            0,
            d.text(
                "Los preajustes respetan la proporción.",
                "Presets respect the aspect ratio lock.",
            ),
            [333, 151, 283, 21],
        );

        radio(
            d,
            hwnd,
            QUALITY,
            d.text("Remuestrear con filtro", "Resample with filter"),
            true,
            [333, 204, 280, 20],
        );
        d.combo(
            hwnd,
            FILTER,
            &[
                "Lanczos",
                d.text("Bicúbico", "Bicubic"),
                d.text("Bilineal", "Bilinear"),
                d.text("Gaussiano", "Gaussian"),
                d.text("Promedio de área", "Area averaging"),
            ],
            0,
            [347, 229, 269, 150],
        );
        radio(
            d,
            hwnd,
            FAST,
            d.text("Redimensionar rápido", "Fast resize"),
            false,
            [333, 258, 280, 20],
        );
        d.checkbox(
            hwnd,
            FAST_REDUCE,
            d.text("Filtro rápido al reducir", "Use fast filter when shrinking"),
            true,
            [333, 282, 282, 20],
        );
        d.checkbox(
            hwnd,
            GAMMA,
            d.text("Mejorar gamma de remuestreo", "Gamma-correct resampling"),
            false,
            [333, 306, 282, 20],
        );
        d.checkbox(
            hwnd,
            SHARPEN,
            d.text("Aplicar nitidez después", "Sharpen afterwards"),
            false,
            [333, 330, 282, 20],
        );

        d.label(hwnd, STATUS, "", [10, 366, 618, 18]);
        d.divider(hwnd, 8, 388, 622);
        d.button(
            hwnd,
            ACCEPT,
            d.text("Aceptar", "OK"),
            BS_DEFPUSHBUTTON,
            [446, 394, 86, 22],
        );
        d.button(
            hwnd,
            CANCEL,
            d.text("Cancelar", "Cancel"),
            BS_PUSHBUTTON,
            [544, 394, 86, 22],
        );
        self.refresh(hwnd, d);
        unsafe {
            SendMessageW(width, EM_SETSEL, None, Some(LPARAM(-1)));
        }
        width
    }

    pub fn command(&mut self, hwnd: HWND, d: &Dialog, id: i32, notification: u32) -> bool {
        if id == ACCEPT {
            self.result = self.operation(hwnd);
            return self.result.is_some();
        }
        if matches!(id, MODE | UNITS) && notification == CBN_SELCHANGE {
            let previous = self.target(hwnd).unwrap_or(self.original);
            self.mode = [Mode::Dimensions, Mode::Percent, Mode::Megapixels][selected(hwnd, MODE)];
            self.unit = [Unit::Pixels, Unit::Centimeters, Unit::Inches][selected(hwnd, UNITS)];
            if self.physical() {
                set_text(control(hwnd, DPI), &self.manual_dpi);
            }
            self.set_size(hwnd, previous);
        } else if id == STANDARD && notification == CBN_SELCHANGE {
            if let Some(index) = selected(hwnd, STANDARD).checked_sub(1) {
                let size = STANDARD_SIZES
                    .get(index)
                    .map(|(_, size)| *size)
                    .or_else(|| self.custom.get(index - STANDARD_SIZES.len()).copied());
                if let Some(size) = size {
                    let size = if checked(hwnd, LOCK) {
                        fit_inside(self.original, size)
                    } else {
                        size
                    };
                    self.set_size(hwnd, size);
                }
            }
        } else if matches!(id, HALF | DOUBLE | SWAP | FIT_DESKTOP | EXACT_DESKTOP) {
            let (w, h) = self.target(hwnd).unwrap_or(self.original);
            let size = match id {
                HALF => (w.div_ceil(2), h.div_ceil(2)),
                DOUBLE => (w.saturating_mul(2), h.saturating_mul(2)),
                SWAP => (h, w),
                FIT_DESKTOP => fit_inside(self.original, self.desktop),
                _ => self.desktop,
            };
            if matches!(id, SWAP | EXACT_DESKTOP) {
                set_check(hwnd, LOCK, false);
                self.mode = Mode::Dimensions;
                self.unit = Unit::Pixels;
                send(control(hwnd, MODE), CB_SETCURSEL, 0);
                send(control(hwnd, UNITS), CB_SETCURSEL, 0);
            }
            self.set_size(hwnd, size);
            send(control(hwnd, STANDARD), CB_SETCURSEL, 0);
        } else if id == SAVE_PRESET {
            if let Some(size) = self.target(hwnd) {
                store_preset(REGISTRY, &mut self.custom, size);
                self.update_presets(hwnd, d);
                send(
                    control(hwnd, STANDARD),
                    CB_SETCURSEL,
                    STANDARD_SIZES.len() + 1,
                );
            }
        } else if matches!(id, QUALITY | FAST) {
            set_check(hwnd, QUALITY, id == QUALITY);
            set_check(hwnd, FAST, id == FAST);
        } else if matches!(id, LOCK | DPI | ADJUST_DPI | WIDTH | HEIGHT) {
            if id == DPI && notification == EN_CHANGE {
                self.manual_dpi = value(hwnd, DPI);
            }
            if id == ADJUST_DPI && !checked(hwnd, ADJUST_DPI) {
                set_text(control(hwnd, DPI), &self.manual_dpi);
            }
            if matches!(id, WIDTH | HEIGHT) && notification == EN_CHANGE {
                self.last_axis = id;
            }
            self.sync_axes(hwnd);
        }
        self.refresh(hwnd, d);
        false
    }

    fn update_presets(&self, hwnd: HWND, d: &Dialog) {
        let built_in = STANDARD_SIZES.map(|(label, _)| label);
        preset_combo(
            hwnd,
            STANDARD,
            d.text("Elegir tamaño predefinido…", "Choose a standard size…"),
            &built_in,
            &self.custom,
        );
    }

    fn physical(&self) -> bool {
        self.mode == Mode::Dimensions && self.unit.is_physical()
    }

    fn dpi(&self, hwnd: HWND) -> Option<(f64, f64)> {
        let dpi = number(hwnd, DPI).filter(|n| (0.01..=100_000.0).contains(n))?;
        if (dpi - self.original_dpi.0).abs() < 0.000001 {
            Some(self.original_dpi)
        } else {
            Some((dpi, dpi))
        }
    }

    fn axes_multiplier(&self, hwnd: HWND) -> Option<(f64, f64)> {
        match self.mode {
            Mode::Percent => Some((
                f64::from(self.original.0) / 100.0,
                f64::from(self.original.1) / 100.0,
            )),
            Mode::Dimensions if self.physical() => {
                let (x, y) = self.dpi(hwnd)?;
                Some((self.unit.pixels_per_unit(x), self.unit.pixels_per_unit(y)))
            }
            _ => Some((1.0, 1.0)),
        }
    }

    fn target(&self, hwnd: HWND) -> Option<(u32, u32)> {
        let (w, h) = (f64::from(self.original.0), f64::from(self.original.1));
        if self.mode == Mode::Megapixels {
            let requested = number(hwnd, MP)?;
            let scale = (requested * 1e6 / (w * h)).sqrt();
            return checked_size(w * scale, h * scale);
        }
        let (kx, ky) = self.axes_multiplier(hwnd)?;
        if checked(hwnd, LOCK) {
            if self.last_axis == WIDTH {
                let target = number(hwnd, WIDTH)? * kx;
                checked_size(target, target * h / w)
            } else {
                let target = number(hwnd, HEIGHT)? * ky;
                checked_size(target * w / h, target)
            }
        } else {
            checked_size(number(hwnd, WIDTH)? * kx, number(hwnd, HEIGHT)? * ky)
        }
    }

    fn set_size(&mut self, hwnd: HWND, size: (u32, u32)) {
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        if self.mode == Mode::Megapixels {
            set_number(hwnd, MP, decimal(w * h / 1e6));
        } else if let Some((kx, ky)) = self.axes_multiplier(hwnd) {
            self.last_axis = if u64::from(size.0) * u64::from(self.original.1)
                > u64::from(size.1) * u64::from(self.original.0)
            {
                HEIGHT
            } else {
                WIDTH
            };
            set_number(hwnd, WIDTH, decimal(w / kx));
            set_number(hwnd, HEIGHT, decimal(h / ky));
            self.sync_axes(hwnd);
        }
    }

    fn sync_axes(&self, hwnd: HWND) {
        if !checked(hwnd, LOCK) || self.mode == Mode::Megapixels {
            return;
        }
        let Some((kx, ky)) = self.axes_multiplier(hwnd) else {
            return;
        };
        let (w, h) = (f64::from(self.original.0), f64::from(self.original.1));
        if self.last_axis == WIDTH {
            if let Some(value) = number(hwnd, WIDTH).filter(|n| *n > 0.0) {
                self.set_other(hwnd, HEIGHT, value * kx * h / w / ky);
            }
        } else if let Some(value) = number(hwnd, HEIGHT).filter(|n| *n > 0.0) {
            self.set_other(hwnd, WIDTH, value * ky * w / h / kx);
        }
    }

    fn set_other(&self, hwnd: HWND, id: i32, number: f64) {
        let number = if self.mode == Mode::Dimensions && self.unit == Unit::Pixels {
            number.round().max(1.0)
        } else {
            number
        };
        set_number(hwnd, id, decimal(number));
    }

    fn output_dpi(&self, hwnd: HWND, size: (u32, u32)) -> Option<(f64, f64)> {
        if !self.physical() && checked(hwnd, ADJUST_DPI) {
            Some((
                self.original_dpi.0 * f64::from(size.0) / f64::from(self.original.0),
                self.original_dpi.1 * f64::from(size.1) / f64::from(self.original.1),
            ))
        } else {
            self.dpi(hwnd)
        }
    }

    fn operation(&self, hwnd: HWND) -> Option<EditOperation> {
        let size = self.target(hwnd)?;
        let dpi = self.output_dpi(hwnd, size)?;
        let dpi = (dpi.0.clamp(0.01, 100_000.0), dpi.1.clamp(0.01, 100_000.0));
        let change_dpi = (dpi.0 - self.original_dpi.0).abs() >= 0.005
            || (dpi.1 - self.original_dpi.1).abs() >= 0.005;
        if size == self.original && !change_dpi {
            return None;
        }
        let fast = checked(hwnd, FAST);
        let reducing =
            size.0 <= self.original.0 && size.1 <= self.original.1 && size != self.original;
        let linear_light = !fast && checked(hwnd, GAMMA);
        let filter = if fast {
            ResampleFilter::Nearest
        } else if reducing && checked(hwnd, FAST_REDUCE) {
            ResampleFilter::Area
        } else {
            FILTERS[selected(hwnd, FILTER)]
        };
        Some(EditOperation::Resize {
            size,
            options: ResizeOptions {
                filter,
                linear_light,
                sharpen: !fast && checked(hwnd, SHARPEN),
                dpi: change_dpi.then(|| {
                    (
                        (dpi.0 * 100.0).round() as u32,
                        (dpi.1 * 100.0).round() as u32,
                    )
                }),
            },
        })
    }

    fn refresh(&self, hwnd: HWND, d: &Dialog) {
        let is_mp = self.mode == Mode::Megapixels;
        let quality = !checked(hwnd, FAST);
        for id in [WIDTH, HEIGHT] {
            enable(hwnd, id, !is_mp);
        }
        enable(hwnd, LOCK, !is_mp);
        enable(hwnd, UNITS, self.mode == Mode::Dimensions);
        enable(hwnd, MP, is_mp);
        enable(hwnd, ADJUST_DPI, !self.physical());
        let adjust = !self.physical() && checked(hwnd, ADJUST_DPI);
        enable(hwnd, DPI, !adjust);
        for id in [FILTER, FAST_REDUCE, GAMMA, SHARPEN] {
            enable(hwnd, id, quality);
        }
        let target = self.target(hwnd);
        let dpi = target.and_then(|size| self.output_dpi(hwnd, size));
        if adjust && let Some((dpi, _)) = dpi {
            set_number(hwnd, DPI, decimal(dpi));
        }
        let result = target
            .map(|size| format!("{} px", dimensions(size)))
            .unwrap_or_else(|| "—".into());
        set_number(
            hwnd,
            SUMMARY,
            format!("{}  {result}", d.text("Resultado", "Result")),
        );
        set_text(
            control(hwnd, STATUS),
            if target.is_none() {
                d.text(
                    "Dimensiones no válidas o fuera del límite.",
                    "Invalid dimensions or dimensions exceed the limit.",
                )
            } else if dpi.is_none() {
                d.text("Introduce un valor DPI válido.", "Enter a valid DPI value.")
            } else {
                ""
            },
        );
        enable(hwnd, SAVE_PRESET, target.is_some());
        enable(hwnd, ACCEPT, self.operation(hwnd).is_some());
    }
}

fn checked_size(width: f64, height: f64) -> Option<(u32, u32)> {
    let dimensions = (rounded(width)?, rounded(height)?);
    editable_size(dimensions).then_some(dimensions)
}

fn fit_inside(original: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(bounds.0) / f64::from(original.0))
        .min(f64::from(bounds.1) / f64::from(original.1));
    (
        (f64::from(original.0) * scale).round().max(1.0) as u32,
        (f64::from(original.1) * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_dimensions_and_monitor_fit_preserve_expected_scale() {
        assert_eq!(
            checked_size(
                5.08 * Unit::Centimeters.pixels_per_unit(300.0),
                1.0 * Unit::Inches.pixels_per_unit(300.0)
            ),
            Some((600, 300))
        );
        assert_eq!(fit_inside((200, 300), (1920, 1080)), (720, 1080));
    }
}
