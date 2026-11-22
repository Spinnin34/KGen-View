use std::{env, fs, path::PathBuf};

#[path = "build/filetypes.rs"]
mod filetypes;
#[path = "src/formats.rs"]
mod formats;

fn main() {
    println!("cargo:rerun-if-changed=resources/app.manifest");
    println!("cargo:rerun-if-changed=resources/kgen-view-logo.png");
    println!("cargo:rerun-if-changed=resources/toolbar");
    println!("cargo:rerun-if-changed=resources/filetype_pack");
    println!("cargo:rerun-if-env-changed=KGEN_BUILD_ID");

    let version = env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let mut components = version.split(['.', '-', '+']);
    let version_numbers = [
        components.next().unwrap_or("1").parse::<u32>().unwrap_or(1),
        components.next().unwrap_or("0").parse::<u32>().unwrap_or(0),
        components.next().unwrap_or("0").parse::<u32>().unwrap_or(0),
    ];
    let build_id = env::var("KGEN_BUILD_ID").unwrap_or_else(|_| "local".to_string());
    if build_id.is_empty()
        || !build_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        panic!("KGEN_BUILD_ID must contain only ASCII letters, digits, '-' or '_'");
    }
    println!("cargo:rustc-env=KGEN_BUILD_ID={build_id}");

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
    fs::create_dir_all(package_icon.parent().unwrap()).expect("create package resources directory");
    fs::write(&package_icon, &icon_bytes).expect("write package icon");
    let filetype_dir = package_icon.parent().unwrap().join("filetype-icons");
    fs::create_dir_all(&filetype_dir).expect("create filetype icon directory");
    let pack_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("resources")
        .join("filetype_pack");
    filetypes::build_filetype_icons(&pack_dir, &filetype_dir);

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
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
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
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "KGenView"
      VALUE "OriginalFilename", "KGenView.exe"
      VALUE "ProductName", "KGen View"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
1 24 "{manifest}"
"#,
            major = version_numbers[0],
            minor = version_numbers[1],
            patch = version_numbers[2],
            version = version,
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
    const NAMES: [&str; 11] = [
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
        "download",
    ];
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("resources")
        .join("toolbar");
    let mut atlas = Vec::with_capacity((SIZE * SIZE * 4 * NAMES.len() as u32) as usize);
    let decoded: Vec<_> = NAMES
        .into_iter()
        .map(|name| {
            let source = if name == "download" {
                build_download_icon()
            } else {
                image::open(root.join(format!("{name}.png")))
                    .unwrap_or_else(|error| panic!("decode toolbar icon {name}: {error}"))
                    .into_rgba8()
            };
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

fn build_download_icon() -> image::RgbaImage {
    use image::{Rgba, RgbaImage};

    const SIZE: u32 = 128;
    const CORNER: u32 = 17;
    let mut image = RgbaImage::new(SIZE, SIZE);
    let background = Rgba([184, 255, 133, 255]);
    let foreground = Rgba([65, 105, 64, 255]);

    for y in 0..SIZE {
        for x in 0..SIZE {
            let center_x = if x < CORNER {
                CORNER
            } else if x >= SIZE - CORNER {
                SIZE - CORNER - 1
            } else {
                x
            };
            let center_y = if y < CORNER {
                CORNER
            } else if y >= SIZE - CORNER {
                SIZE - CORNER - 1
            } else {
                y
            };
            let dx = x as i32 - center_x as i32;
            let dy = y as i32 - center_y as i32;
            if dx * dx + dy * dy <= (CORNER * CORNER) as i32 {
                image.put_pixel(x, y, background);
            }
        }
    }

    draw_icon_stroke(&mut image, (64.0, 30.0), (64.0, 82.0), 8.0, foreground);
    draw_icon_stroke(&mut image, (43.0, 58.0), (64.0, 83.0), 8.0, foreground);
    draw_icon_stroke(&mut image, (85.0, 58.0), (64.0, 83.0), 8.0, foreground);
    draw_icon_stroke(&mut image, (26.0, 98.0), (102.0, 98.0), 8.0, foreground);
    image
}

fn draw_icon_stroke(
    image: &mut image::RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    width: f32,
    color: image::Rgba<u8>,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let length_squared = dx * dx + dy * dy;
    let radius = width * 0.5;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let projection =
                (((px - start.0) * dx + (py - start.1) * dy) / length_squared).clamp(0.0, 1.0);
            let nearest_x = start.0 + projection * dx;
            let nearest_y = start.1 + projection * dy;
            let offset_x = px - nearest_x;
            let offset_y = py - nearest_y;
            if offset_x * offset_x + offset_y * offset_y <= radius * radius {
                image.put_pixel(x, y, color);
            }
        }
    }
}

fn build_icon(logo: &std::path::Path) -> Vec<u8> {
    let source = image::open(logo)
        .expect("decode KGen View logo")
        .into_rgba8();
    encode_scaled_icon(&source)
}

fn encode_scaled_icon(source: &image::RgbaImage) -> Vec<u8> {
    use image::imageops::FilterType;

    let images = icon_sizes()
        .map(|size| {
            let resized = image::imageops::resize(source, size, size, FilterType::Lanczos3);
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
