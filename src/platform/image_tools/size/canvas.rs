use super::*;
use crate::imaging::CanvasPaint;

pub(in crate::platform::image_tools) const CANVAS_CLIENT: (i32, i32) = (636, 402);
const REGISTRY: &str = "CanvasDimensions";
const CURRENT: i32 = 200;
const SUMMARY: i32 = 201;
const STATUS: i32 = 202;
const METHOD: i32 = 203;
const WIDTH: i32 = 204;
const HEIGHT: i32 = 205;
const INSIDE: i32 = 206;
const CLEAR: i32 = 207;
const RATIO: i32 = 208;
const CUSTOM: i32 = 209;
const BLUR: i32 = 210;
const STRENGTH: i32 = 211;
const FLIP: i32 = 212;
const GAMMA: i32 = 213;
const TRANSPARENT_FILL: i32 = 214;
const COLOR: i32 = 215;
const CHOOSE: i32 = 216;
const SAMPLE: i32 = 217;
const PRESETS: i32 = 218;
const SAVE: i32 = 219;
const APPLY: i32 = 220;
const UNDO: i32 = 221;
const MARGINS: [i32; 4] = [222, 223, 224, 225];
const ANCHOR: i32 = 230;
const RATIOS: [(&str, f64); 7] = [
    ("1:1", 1.0),
    ("2:1", 2.0),
    ("3:2", 1.5),
    ("4:3", 4.0 / 3.0),
    ("16:9", 16.0 / 9.0),
    ("16:10", 1.6),
    ("1.4142", std::f64::consts::SQRT_2),
];

#[derive(Debug, PartialEq, Eq)]
struct Geometry {
    size: (u32, u32),
    offset: (i32, i32),
    inset: [u32; 4],
}

pub(in crate::platform::image_tools) struct CanvasPanel {
    original: (u32, u32),
    presets: Vec<(u32, u32)>,
    operation: Option<EditOperation>,
    applying: bool,
    close_after_apply: bool,
    undo_count: usize,
    picking: bool,
}

impl CanvasPanel {
    pub fn new(original: (u32, u32)) -> Self {
        Self {
            original,
            presets: read_presets(REGISTRY),
            operation: None,
            applying: false,
            close_after_apply: false,
            undo_count: 0,
            picking: false,
        }
    }

    pub fn create(&self, hwnd: HWND, d: &Dialog) -> HWND {
        d.label(hwnd, CURRENT, "", [10, 9, 304, 18]);
        d.label(hwnd, SUMMARY, "", [325, 9, 302, 18]);
        d.group(
            hwnd,
            d.text("Dimensiones del lienzo", "Canvas dimensions"),
            [8, 33, 305, 201],
        );
        d.group(
            hwnd,
            d.text("Anclaje de la imagen", "Image anchor"),
            [8, 240, 305, 104],
        );
        d.group(
            hwnd,
            d.text("Relleno de la zona nueva", "Fill the new area"),
            [322, 33, 306, 201],
        );
        d.group(
            hwnd,
            d.text("Dimensiones guardadas", "Saved dimensions"),
            [322, 240, 306, 104],
        );
        let method = d.combo(
            hwnd,
            METHOD,
            &[
                d.text("Márgenes por lado", "Margins per side"),
                d.text("Anchura y altura finales", "Final width and height"),
                d.text("Relación de aspecto", "Aspect ratio"),
            ],
            1,
            [20, 51, 276, 114],
        );
        for (i, (id, label)) in MARGINS
            .into_iter()
            .zip([
                d.text("Izquierda", "Left"),
                d.text("Arriba", "Top"),
                d.text("Derecha", "Right"),
                d.text("Abajo", "Bottom"),
            ])
            .enumerate()
        {
            let x = if i < 2 { 20 } else { 152 };
            let y = 81 + (i % 2) as i32 * 22;
            d.label(hwnd, 0, label, [x, y + 2, 62, 18]);
            d.input(hwnd, id, "0", [x + 64, y, 60, 20]);
        }
        d.checkbox(
            hwnd,
            INSIDE,
            d.text(
                "Negativos como borde interior",
                "Negative values draw inside",
            ),
            true,
            [20, 126, 221, 20],
        );
        d.button(
            hwnd,
            CLEAR,
            d.text("Limpiar", "Clear"),
            BS_PUSHBUTTON,
            [243, 126, 56, 20],
        );
        d.divider(hwnd, 20, 151, 277);
        d.label(hwnd, 0, d.text("Ancho", "Width"), [20, 164, 42, 18]);
        d.input(hwnd, WIDTH, &self.original.0.to_string(), [66, 161, 57, 20]);
        d.label(hwnd, 0, d.text("Alto", "Height"), [132, 164, 33, 18]);
        d.input(
            hwnd,
            HEIGHT,
            &self.original.1.to_string(),
            [169, 161, 57, 20],
        );
        d.label(
            hwnd,
            0,
            d.text("0 = original", "0 = original"),
            [232, 164, 69, 18],
        );
        let ratio = RATIOS
            .iter()
            .map(|(name, _)| *name)
            .chain([d.text("Proporción personalizada", "Custom ratio")])
            .collect::<Vec<_>>();
        d.combo(hwnd, RATIO, &ratio, 0, [20, 191, 194, 172]);
        d.input(hwnd, CUSTOM, "1.00", [223, 191, 76, 20]);

        for (i, label) in [
            d.text("Sup. izq.", "Top left"),
            d.text("Sup. centro", "Top middle"),
            d.text("Sup. der.", "Top right"),
            d.text("Centro izq.", "Mid. left"),
            d.text("Centro", "Center"),
            d.text("Centro der.", "Mid. right"),
            d.text("Inf. izq.", "Bot. left"),
            d.text("Inf. centro", "Bot. middle"),
            d.text("Inf. der.", "Bot. right"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut style = WINDOW_STYLE(BS_AUTORADIOBUTTON as u32);
            if i == 0 {
                style |= WS_GROUP;
            }
            if i == 4 {
                style |= WS_TABSTOP;
            }
            d.radio(
                hwnd,
                ANCHOR + i as i32,
                label,
                style,
                [20 + (i % 3) as i32 * 95, 260 + (i / 3) as i32 * 24, 95, 20],
            );
        }
        set_check(hwnd, ANCHOR + 4, true);

        d.checkbox(
            hwnd,
            BLUR,
            d.text("Rellenar con imagen desenfocada", "Fill with blurred image"),
            false,
            [334, 55, 286, 20],
        );
        d.label(
            hwnd,
            0,
            d.text("Intensidad (1–199)", "Strength (1–199)"),
            [337, 83, 112, 18],
        );
        d.input(hwnd, STRENGTH, "50", [453, 80, 48, 20]);
        d.checkbox(
            hwnd,
            FLIP,
            d.text("Voltear relleno", "Mirror fill"),
            false,
            [337, 106, 258, 20],
        );
        d.label(
            hwnd,
            0,
            d.text("Gamma de relleno (0,01–6,99)", "Fill gamma (0.01–6.99)"),
            [337, 132, 211, 18],
        );
        d.input(hwnd, GAMMA, "1.00", [552, 129, 62, 20]);
        d.divider(hwnd, 335, 155, 280);
        d.checkbox(
            hwnd,
            TRANSPARENT_FILL,
            d.text("Relleno transparente", "Transparent fill"),
            true,
            [337, 163, 272, 20],
        );
        d.input(hwnd, COLOR, "#000000", [337, 190, 80, 22]);
        d.button(
            hwnd,
            CHOOSE,
            d.text("Elegir color…", "Choose color…"),
            BS_PUSHBUTTON,
            [422, 190, 91, 22],
        );
        d.button(
            hwnd,
            SAMPLE,
            d.text("Cuentagotas", "Sample image"),
            BS_PUSHBUTTON,
            [519, 190, 95, 22],
        );

        d.combo(hwnd, PRESETS, &[], 0, [336, 260, 278, 190]);
        self.update_presets(hwnd, d);
        d.button(
            hwnd,
            SAVE,
            d.text("Guardar dimensiones actuales", "Save current dimensions"),
            BS_PUSHBUTTON,
            [336, 291, 278, 23],
        );
        d.label(hwnd, STATUS, "", [10, 350, 618, 19]);
        d.divider(hwnd, 8, 372, 620);
        d.button(
            hwnd,
            APPLY,
            d.text("Aplicar a la imagen", "Apply to image"),
            BS_PUSHBUTTON,
            [8, 378, 129, 22],
        );
        d.button(
            hwnd,
            UNDO,
            d.text("Deshacer", "Undo"),
            BS_PUSHBUTTON,
            [144, 378, 86, 22],
        );
        d.button(
            hwnd,
            ACCEPT,
            d.text("Aceptar", "OK"),
            BS_DEFPUSHBUTTON,
            [443, 378, 86, 22],
        );
        d.button(
            hwnd,
            CANCEL,
            d.text("Cancelar", "Cancel"),
            BS_PUSHBUTTON,
            [542, 378, 86, 22],
        );
        self.refresh(hwnd, d);
        method
    }

    pub fn command(&mut self, hwnd: HWND, d: &Dialog, id: i32, notification: u32) -> bool {
        if matches!(id, APPLY | ACCEPT) && !self.applying {
            if let Ok(Some(operation)) = self.read_operation(hwnd) {
                self.operation = Some(operation);
                self.close_after_apply = id == ACCEPT;
                self.applying = true;
                d.notify(hwnd, APPLY_CANVAS);
            } else if id == ACCEPT && self.read_operation(hwnd) == Ok(None) {
                return true;
            }
        } else if id == UNDO && !self.applying && self.undo_count > 0 {
            self.applying = true;
            d.notify(hwnd, UNDO_CANVAS);
        } else if id == CLEAR {
            for id in MARGINS {
                set_number(hwnd, id, 0);
            }
        } else if id == SAMPLE {
            self.picking = !self.picking;
            if self.picking {
                d.notify(hwnd, PICK_CANVAS_COLOR);
            } else {
                d.notify(hwnd, CANCEL_CANVAS_PICK);
            }
        } else if id == CHOOSE {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CANVAS_PALETTE, WPARAM(0), LPARAM(0));
            }
        } else if id == SAVE {
            if let Some(geometry) = self.geometry(hwnd) {
                store_preset(REGISTRY, &mut self.presets, geometry.size);
                self.update_presets(hwnd, d);
                send(control(hwnd, PRESETS), CB_SETCURSEL, 1);
            }
        } else if id == PRESETS && notification == CBN_SELCHANGE {
            let index = selected(hwnd, PRESETS);
            if let Some(&(w, h)) = index.checked_sub(1).and_then(|i| self.presets.get(i)) {
                set_number(hwnd, WIDTH, w);
                set_number(hwnd, HEIGHT, h);
                send(control(hwnd, METHOD), CB_SETCURSEL, 1);
            }
        }
        self.refresh(hwnd, d);
        false
    }

    pub fn pending(&mut self) -> Option<EditOperation> {
        self.operation.take()
    }

    pub fn finish(&mut self, hwnd: HWND, d: &Dialog, size: Option<(u32, u32)>, undo: bool) -> bool {
        self.applying = false;
        if let Some(size) = size {
            if !undo && self.close_after_apply {
                return true;
            }
            self.undo_count = if undo {
                self.undo_count.saturating_sub(1)
            } else {
                self.undo_count + 1
            };
            let chosen = selected(hwnd, METHOD);
            for id in MARGINS {
                set_number(hwnd, id, 0);
            }
            if chosen == 1 || undo {
                set_number(hwnd, WIDTH, size.0);
                set_number(hwnd, HEIGHT, size.1);
            }
            self.original = size;
        }
        self.close_after_apply = false;
        self.refresh(hwnd, d);
        false
    }

    pub fn picked_color(&mut self, hwnd: HWND, d: &Dialog, color: Option<[u8; 3]>) {
        self.picking = false;
        if let Some([red, green, blue]) = color {
            set_check(hwnd, TRANSPARENT_FILL, false);
            set_check(hwnd, BLUR, false);
            set_number(hwnd, COLOR, format!("#{red:02X}{green:02X}{blue:02X}"));
        }
        self.refresh(hwnd, d);
    }

    fn update_presets(&self, hwnd: HWND, d: &Dialog) {
        preset_combo(
            hwnd,
            PRESETS,
            d.text("Elegir dimensiones…", "Choose dimensions…"),
            &[],
            &self.presets,
        );
    }

    fn geometry(&self, hwnd: HWND) -> Option<Geometry> {
        let anchor = (0..9).find(|i| checked(hwnd, ANCHOR + i)).unwrap_or(4);
        match selected(hwnd, METHOD) {
            0 => {
                let [left, top, right, bottom] =
                    MARGINS.map(|id| value(hwnd, id).trim().parse::<i32>().ok());
                margins(
                    self.original,
                    [left?, top?, right?, bottom?],
                    checked(hwnd, INSIDE),
                )
            }
            1 => {
                let read = |id, original| {
                    value(hwnd, id)
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .map(|n| if n == 0 { original } else { n })
                };
                anchored(
                    self.original,
                    (
                        read(WIDTH, self.original.0)?,
                        read(HEIGHT, self.original.1)?,
                    ),
                    anchor,
                )
            }
            _ => {
                let ratio = RATIOS
                    .get(selected(hwnd, RATIO))
                    .map(|&(_, value)| value)
                    .or_else(|| number(hwnd, CUSTOM))?;
                if ratio <= 0.0 {
                    return None;
                }
                let (w, h) = self.original;
                let width = rounded((f64::from(h) * ratio).ceil())?.max(w);
                let height = rounded((f64::from(w) / ratio).ceil())?.max(h);
                anchored(self.original, (width, height), anchor)
            }
        }
    }

    fn read_operation(&self, hwnd: HWND) -> Result<Option<EditOperation>, Invalid> {
        let Geometry {
            size,
            offset,
            inset,
        } = self.geometry(hwnd).ok_or(Invalid::Dimensions)?;
        if size == self.original && offset == (0, 0) && inset == [0; 4] {
            return Ok(None);
        }
        let gamma = number(hwnd, GAMMA)
            .filter(|n| (0.01..=6.99).contains(n))
            .ok_or(Invalid::Gamma)?;
        let mut paint = CanvasPaint::solid([0; 4]);
        paint.gamma = (gamma * 100.0).round() as u16;
        if checked(hwnd, BLUR) {
            let strength = value(hwnd, STRENGTH)
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=199).contains(n))
                .ok_or(Invalid::Blur)?;
            paint.blur = Some((strength, checked(hwnd, FLIP)));
        } else if !checked(hwnd, TRANSPARENT_FILL) {
            let [r, g, b] = read_rgb(&value(hwnd, COLOR)).ok_or(Invalid::Color)?;
            paint.color = [b, g, r, 255];
        }
        Ok(Some(EditOperation::Canvas {
            size,
            offset,
            inset,
            paint,
        }))
    }

    fn refresh(&self, hwnd: HWND, d: &Dialog) {
        let method = selected(hwnd, METHOD);
        let blurred = checked(hwnd, BLUR);
        let transparent = checked(hwnd, TRANSPARENT_FILL);
        for id in MARGINS.into_iter().chain([CLEAR, INSIDE]) {
            enable(hwnd, id, method == 0);
        }
        for id in [WIDTH, HEIGHT] {
            enable(hwnd, id, method == 1);
        }
        for id in ANCHOR..ANCHOR + 9 {
            enable(hwnd, id, method != 0);
        }
        enable(hwnd, RATIO, method == 2);
        enable(
            hwnd,
            CUSTOM,
            method == 2 && selected(hwnd, RATIO) == RATIOS.len(),
        );
        enable(hwnd, STRENGTH, blurred);
        enable(hwnd, FLIP, blurred);
        enable(hwnd, TRANSPARENT_FILL, !blurred);
        enable(hwnd, COLOR, !blurred && !transparent);
        enable(hwnd, CHOOSE, !blurred);
        enable(hwnd, SAMPLE, !blurred);
        enable(hwnd, SAVE, self.geometry(hwnd).is_some());
        enable(hwnd, UNDO, self.undo_count > 0 && !self.applying);
        let operation = self.read_operation(hwnd);
        enable(hwnd, ACCEPT, operation.is_ok() && !self.applying);
        enable(
            hwnd,
            APPLY,
            matches!(operation, Ok(Some(_))) && !self.applying,
        );
        set_number(
            hwnd,
            CURRENT,
            format!(
                "{}  {} px",
                d.text("Actual", "Current"),
                dimensions(self.original)
            ),
        );
        let preview = self
            .geometry(hwnd)
            .map(|g| format!("{} px", dimensions(g.size)))
            .unwrap_or_else(|| "—".into());
        set_number(
            hwnd,
            SUMMARY,
            format!("{}  {preview}", d.text("Resultado", "Result")),
        );
        let status = if self.picking {
            d.text(
                "Haz clic en un píxel de la imagen principal para tomar su color.",
                "Click a pixel in the main image to sample its color.",
            )
        } else if self.applying {
            d.text("Procesando lienzo…", "Processing canvas…")
        } else {
            match operation {
                Ok(_) => "",
                Err(Invalid::Dimensions) => d.text(
                    "Dimensiones fuera del límite o no válidas.",
                    "Invalid dimensions or dimensions exceed the limit.",
                ),
                Err(Invalid::Color) => d.text(
                    "Escribe un color hexadecimal de seis dígitos.",
                    "Enter a six-digit hex color.",
                ),
                Err(Invalid::Gamma) => d.text(
                    "Gamma debe estar entre 0,01 y 6,99.",
                    "Gamma must be between 0.01 and 6.99.",
                ),
                Err(Invalid::Blur) => d.text(
                    "La intensidad debe ser un entero entre 1 y 199.",
                    "Strength must be an integer from 1 to 199.",
                ),
            }
        };
        set_text(control(hwnd, STATUS), status);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Invalid {
    Dimensions,
    Color,
    Gamma,
    Blur,
}

fn margins(original: (u32, u32), edges: [i32; 4], inside: bool) -> Option<Geometry> {
    let inset = edges.map(|edge| {
        if inside && edge < 0 {
            edge.unsigned_abs()
        } else {
            0
        }
    });
    let external = edges.map(|edge| if inside { edge.max(0) } else { edge });
    let [left, top, right, bottom] = external.map(i64::from);
    let size = (
        u32::try_from(i64::from(original.0) + left + right).ok()?,
        u32::try_from(i64::from(original.1) + top + bottom).ok()?,
    );
    editable_size(size).then_some(Geometry {
        size,
        offset: (external[0], external[1]),
        inset,
    })
}

fn anchored(original: (u32, u32), size: (u32, u32), anchor: i32) -> Option<Geometry> {
    let align = |difference: i32, axis| match axis {
        0 => 0,
        1 => difference.div_euclid(2),
        _ => difference,
    };
    editable_size(size).then(|| Geometry {
        size,
        offset: (
            align(size.0 as i32 - original.0 as i32, anchor % 3),
            align(size.1 as i32 - original.1 as i32, anchor / 3),
        ),
        inset: [0; 4],
    })
}

pub(in crate::platform::image_tools) fn choose_color(hwnd: HWND) -> Option<[u8; 3]> {
    use windows::Win32::UI::Controls::Dialogs::{
        CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW,
    };
    let mut custom = [COLORREF(0x00ffffff); 16];
    let [r, g, b] = read_rgb(&value(hwnd, COLOR)).unwrap_or([0; 3]);
    let mut chooser = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if unsafe { ChooseColorW(&mut chooser) }.as_bool() {
        let color = chooser.rgbResult.0;
        Some([color as u8, (color >> 8) as u8, (color >> 16) as u8])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_margins_can_crop_or_fill_inside() {
        let edges = [-10, 12, 0, -7];
        assert_eq!(
            margins((100, 80), edges, false).unwrap(),
            Geometry {
                size: (90, 85),
                offset: (-10, 12),
                inset: [0; 4]
            }
        );
        assert_eq!(
            margins((100, 80), edges, true).unwrap(),
            Geometry {
                size: (100, 92),
                offset: (0, 12),
                inset: [10, 0, 0, 7]
            }
        );
    }
}
