use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=resources/app.manifest");
    println!("cargo:rerun-if-changed=resources/kgen-view-logo.png");
    println!("cargo:rerun-if-changed=resources/toolbar");
    println!("cargo:rerun-if-changed=vendor/IrfanView-Filetype-IconsChanger/Icon");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = out.join("kgen-view.ico");
    let rc = out.join("kgen-view.rc");
    let logo = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("resources")
        .join("kgen-view-logo.png");
    let icon_bytes = build_icon(&logo);
    fs::write(&icon, &icon_bytes).expect("write generated icon");
    fs::write(out.join("toolbar-atlas.bgra"), build_toolbar_atlas()).expect("write toolbar atlas");
    let package_icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("target")
        .join("kgen-view.ico");
    fs::write(&package_icon, &icon_bytes).expect("write package icon");
    let filetype_dir = package_icon.parent().unwrap().join("filetype-icons");
    fs::create_dir_all(&filetype_dir).expect("create filetype icon directory");
    let pack_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("resources")
        .join("filetype_pack");
    let vendor_icons = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("vendor")
        .join("IrfanView-Filetype-IconsChanger")
        .join("Icon");
    let _ = fs::create_dir_all(&vendor_icons);

    for name in [
        "bmp", "gif", "heic", "ico", "jpeg", "jpg", "png", "svg", "tiff", "webp",
    ] {
        let png_file = pack_dir.join(format!("{name}.png"));
        if png_file.exists() {
            let ico_bytes = build_filetype_icon_from_png(&png_file);
            let ico_dest = filetype_dir.join(format!("{name}.ico"));
            fs::write(&ico_dest, &ico_bytes).expect("write filetype icon");
            let vendor_dest = vendor_icons.join(format!("{name}.ico"));
            let _ = fs::write(&vendor_dest, &ico_bytes);
        }
    }

    let icon_path = icon.display().to_string().replace('\\', "\\\\");
    let manifest = PathBuf::from("resources/app.manifest")
        .canonicalize()
        .expect("manifest")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    fs::write(
        &rc,
        format!(
            r#"#include <windows.h>
1 ICON "{icon_path}"
1 VERSIONINFO
FILEVERSION 1,2,0,0
PRODUCTVERSION 1,2,0,0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
FILESUBTYPE VFT2_UNKNOWN
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "KGen View"
      VALUE "FileDescription", "KGen View - Visor de imagenes"
      VALUE "FileVersion", "1.2.0"
      VALUE "InternalName", "KGenView"
      VALUE "OriginalFilename", "KGenView.exe"
      VALUE "ProductName", "KGen View"
      VALUE "ProductVersion", "1.2.0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
1 24 "{manifest}"
"#
        ),
    )
    .expect("write generated resource script");

    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

fn build_toolbar_atlas() -> Vec<u8> {
    use image::imageops::FilterType;

    const SIZE: u32 = 48;
    const NAMES: [&str; 10] = [
        "open",
        "save",
        "previous",
        "next",
        "info",
        "copy",
        "copy-url",
        "rotate-left",
        "rotate-right",
        "fullscreen",
    ];
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("resources")
        .join("toolbar");
    let mut atlas = Vec::with_capacity((SIZE * SIZE * 4 * NAMES.len() as u32) as usize);
    let decoded: Vec<_> = NAMES
        .into_iter()
        .map(|name| {
            let source = image::open(root.join(format!("{name}.png")))
                .unwrap_or_else(|error| panic!("decode toolbar icon {name}: {error}"))
                .into_rgba8();
            image::imageops::resize(&source, SIZE, SIZE, FilterType::Lanczos3)
        })
        .collect();
    // The bitmap is one horizontal D2D atlas. D2D expects premultiplied BGRA.
    for y in 0..SIZE {
        for icon in &decoded {
            for x in 0..SIZE {
                let [r, g, b, a] = icon.get_pixel(x, y).0;
                let premultiply =
                    |channel: u8| ((u16::from(channel) * u16::from(a) + 127) / 255) as u8;
                atlas.extend_from_slice(&[premultiply(b), premultiply(g), premultiply(r), a]);
            }
        }
    }
    atlas
}

fn build_icon(logo: &std::path::Path) -> Vec<u8> {
    use image::imageops::FilterType;

    let source = image::open(logo)
        .expect("decode KGen View logo")
        .into_rgba8();
    let images = icon_sizes()
        .map(|size| {
            let resized = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
            (size, resized)
        })
        .collect();
    encode_ico(images)
}

fn build_filetype_icon_from_png(png_path: &std::path::Path) -> Vec<u8> {
    use image::imageops::FilterType;

    let source = image::open(png_path)
        .unwrap_or_else(|error| panic!("decode filetype icon {}: {error}", png_path.display()))
        .into_rgba8();
    let images = icon_sizes()
        .map(|size| {
            let resized = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
            (size, resized)
        })
        .collect();
    encode_ico(images)
}

fn icon_sizes() -> impl Iterator<Item = u32> {
    [16u32, 24, 32, 48, 64, 128, 256].into_iter()
}

fn encode_ico(images: Vec<(u32, image::RgbaImage)>) -> Vec<u8> {
    let images: Vec<_> = images
        .into_iter()
        .map(|(size, pixels)| {
            let row_mask = size.div_ceil(32) * 4;
            let xor_size = size * size * 4;
            let image_size = 40 + xor_size + row_mask * size;
            let mut dib = Vec::with_capacity(image_size as usize);
            dib.extend_from_slice(&40u32.to_le_bytes());
            dib.extend_from_slice(&(size as i32).to_le_bytes());
            dib.extend_from_slice(&((size * 2) as i32).to_le_bytes());
            dib.extend_from_slice(&1u16.to_le_bytes());
            dib.extend_from_slice(&32u16.to_le_bytes());
            dib.extend_from_slice(&0u32.to_le_bytes());
            dib.extend_from_slice(&xor_size.to_le_bytes());
            dib.extend_from_slice(&0i32.to_le_bytes());
            dib.extend_from_slice(&0i32.to_le_bytes());
            dib.extend_from_slice(&0u32.to_le_bytes());
            dib.extend_from_slice(&0u32.to_le_bytes());

            for y_bottom in 0..size {
                let y = size - 1 - y_bottom;
                for x in 0..size {
                    let [r, g, b, a] = pixels.get_pixel(x, y).0;
                    dib.extend_from_slice(&[b, g, r, a]);
                }
            }
            for y_bottom in 0..size {
                let y = size - 1 - y_bottom;
                let row_start = dib.len();
                dib.resize(row_start + row_mask as usize, 0);
                for x in 0..size {
                    if pixels.get_pixel(x, y).0[3] < 8 {
                        dib[row_start + (x / 8) as usize] |= 0x80 >> (x % 8);
                    }
                }
            }
            (size, dib)
        })
        .collect();

    let directory_size = 6 + images.len() * 16;
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = directory_size as u32;
    for (size, dib) in &images {
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(0);
        ico.push(0);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += dib.len() as u32;
    }
    for (_, dib) in images {
        ico.extend_from_slice(&dib);
    }
    ico
}

#[allow(dead_code)]
fn draw_filetype(size: u32, label: &str, color: [u8; 3]) -> image::RgbaImage {
    use image::{Rgba, RgbaImage};

    let mut image = RgbaImage::new(size, size);
    let margin = (size / 12).max(1);
    let radius = (size / 5).max(2);
    let left = margin;
    let top = margin;
    let right = size - margin;
    let bottom = size - margin;
    for y in top..bottom {
        for x in left..right {
            if inside_rounded_rect(x, y, left, top, right, bottom, radius) {
                image.put_pixel(x, y, Rgba([color[0], color[1], color[2], 255]));
            }
        }
    }

    let fold = (size / 4).max(4);
    for y in top..top + fold {
        for x in right.saturating_sub(fold)..right {
            if x + (y - top) >= right {
                image.put_pixel(
                    x,
                    y,
                    Rgba([
                        color[0].saturating_add(34),
                        color[1].saturating_add(34),
                        color[2].saturating_add(34),
                        255,
                    ]),
                );
            }
        }
    }

    let pill_top = top + (bottom - top) * 58 / 100;
    for y in pill_top..bottom {
        for x in left..right {
            if image.get_pixel(x, y).0[3] != 0 {
                image.put_pixel(x, y, Rgba([28, 30, 34, 225]));
            }
        }
    }
    draw_label(&mut image, label, left, pill_top, right, bottom);

    if size >= 24 {
        let sun_x = left + (right - left) * 31 / 100;
        let sun_y = top + (pill_top - top) * 34 / 100;
        let sun_r = (size / 18).max(1);
        for y in sun_y.saturating_sub(sun_r)..=(sun_y + sun_r).min(size - 1) {
            for x in sun_x.saturating_sub(sun_r)..=(sun_x + sun_r).min(size - 1) {
                let dx = x as i32 - sun_x as i32;
                let dy = y as i32 - sun_y as i32;
                if dx * dx + dy * dy <= (sun_r * sun_r) as i32 {
                    image.put_pixel(x, y, Rgba([255, 255, 255, 245]));
                }
            }
        }
        let baseline = pill_top.saturating_sub(2);
        for x in left + 2..right.saturating_sub(2) {
            let relative = (x - left) as f32 / (right - left).max(1) as f32;
            let mountain = if relative < 0.52 {
                top as f32 + (pill_top - top) as f32 * (0.88 - relative * 0.68)
            } else {
                top as f32 + (pill_top - top) as f32 * (0.53 + (relative - 0.52) * 0.73)
            };
            for y in mountain.round() as u32..baseline {
                image.put_pixel(x, y, Rgba([255, 255, 255, 245]));
            }
        }
    }
    image
}

fn inside_rounded_rect(
    x: u32,
    y: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    radius: u32,
) -> bool {
    let near_x = if x < left + radius {
        left + radius
    } else if x >= right.saturating_sub(radius) {
        right.saturating_sub(radius + 1)
    } else {
        x
    };
    let near_y = if y < top + radius {
        top + radius
    } else if y >= bottom.saturating_sub(radius) {
        bottom.saturating_sub(radius + 1)
    } else {
        y
    };
    let dx = x as i64 - near_x as i64;
    let dy = y as i64 - near_y as i64;
    dx * dx + dy * dy <= (radius * radius) as i64
}

fn draw_label(
    image: &mut image::RgbaImage,
    label: &str,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) {
    use image::Rgba;

    const FONT: [[u8; 7]; 15] = [
        [
            0b11110, 0b10000, 0b11100, 0b10000, 0b10000, 0b10000, 0b10000,
        ], // F
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ], // G
        [
            0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b10001,
        ], // H
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ], // I
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ], // J
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ], // P
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ], // M
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ], // T
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ], // W
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ], // R
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ], // O
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ], // B
        [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ], // N
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ], // E
        [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ], // C
    ];
    let glyph = |character: char| match character {
        'F' => FONT[0],
        'G' => FONT[1],
        'H' => FONT[2],
        'I' => FONT[3],
        'J' => FONT[4],
        'P' => FONT[5],
        'M' => FONT[6],
        'T' => FONT[7],
        'W' => FONT[8],
        'R' => FONT[9],
        'O' => FONT[10],
        'B' => FONT[11],
        'N' => FONT[12],
        'E' => FONT[13],
        'C' => FONT[14],
        _ => [0; 7],
    };
    let area_width = right.saturating_sub(left);
    let area_height = bottom.saturating_sub(top);
    let mut text: Vec<_> = label.chars().collect();
    while text.len() > 1 && text.len() as u32 * 6 - 1 > area_width.saturating_sub(2) {
        text.pop();
    }
    let unscaled_width = text.len() as u32 * 6 - 1;
    let scale = (area_height / 9)
        .min(area_width.saturating_sub(4) / unscaled_width.max(1))
        .max(1);
    let text_width = unscaled_width * scale;
    let text_height = 7 * scale;
    let origin_x = left + area_width.saturating_sub(text_width) / 2;
    let origin_y = top + area_height.saturating_sub(text_height) / 2;
    for (index, character) in text.into_iter().enumerate() {
        for (row, bits) in glyph(character).into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let x = origin_x + (index as u32 * 6 + column) * scale + sx;
                        let y = origin_y + row as u32 * scale + sy;
                        if x < image.width() && y < image.height() {
                            image.put_pixel(x, y, Rgba([250, 250, 250, 255]));
                        }
                    }
                }
            }
        }
    }
}
