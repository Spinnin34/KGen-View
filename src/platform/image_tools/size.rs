use super::*;
use crate::imaging::editable_size;

mod canvas;
mod resize;
pub(super) use canvas::choose_color;
pub(super) use canvas::{CANVAS_CLIENT, CanvasPanel};
pub(super) use resize::{RESIZE_CLIENT, ResizePanel};

#[derive(Clone, Copy)]
pub enum SizeTool {
    Canvas,
    Resize,
}

fn number(hwnd: HWND, id: i32) -> Option<f64> {
    let typed = value(hwnd, id)
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()?;
    typed.is_finite().then_some(typed)
}

fn rounded(value: f64) -> Option<u32> {
    (value > 0.0 && value.round() <= 100_000.0).then(|| value.round().max(1.0) as u32)
}

fn decimal(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn set_number(hwnd: HWND, id: i32, number: impl ToString) {
    set_text(control(hwnd, id), &number.to_string());
}

fn set_check(hwnd: HWND, id: i32, checked: bool) {
    send(control(hwnd, id), BM_SETCHECK, usize::from(checked));
}

fn dimensions(size: (u32, u32)) -> String {
    format!("{} × {}", size.0, size.1)
}

fn read_presets(key: &str) -> Vec<(u32, u32)> {
    crate::settings::read_string(key)
        .and_then(|value| serde_json::from_str::<Vec<(u32, u32)>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|&size| editable_size(size))
        .take(24)
        .collect()
}

fn store_preset(key: &str, list: &mut Vec<(u32, u32)>, size: (u32, u32)) {
    list.retain(|&item| item != size);
    list.insert(0, size);
    list.truncate(24);
    if let Ok(json) = serde_json::to_string(list) {
        crate::settings::write_string(key, &json);
    }
}

fn preset_combo(hwnd: HWND, id: i32, placeholder: &str, built_in: &[&str], saved: &[(u32, u32)]) {
    let handle = control(hwnd, id);
    send(handle, CB_RESETCONTENT, 0);
    add_combo_item(handle, placeholder);
    for item in built_in {
        add_combo_item(handle, item);
    }
    for &size in saved {
        add_combo_item(handle, &dimensions(size));
    }
    send(handle, CB_SETCURSEL, 0);
}

fn radio(d: &Dialog, hwnd: HWND, id: i32, label: &str, active: bool, bounds: [i32; 4]) -> HWND {
    let window = d.radio(
        hwnd,
        id,
        label,
        WS_TABSTOP | WINDOW_STYLE(BS_RADIOBUTTON as u32),
        bounds,
    );
    send(window, BM_SETCHECK, usize::from(active));
    window
}

fn read_rgb(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}
