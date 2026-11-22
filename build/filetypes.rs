use std::{fmt::Write as _, fs, path::Path};

use image::{Rgba, RgbaImage};
use resvg::{tiny_skia, usvg};

use crate::{
    encode_scaled_icon,
    formats::{ARCHIVE_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS},
};

pub fn build_filetype_icons(pack: &Path, output: &Path) {
    let mut associations = String::from("\u{feff}");
    for (extensions, family, default_artwork) in [
        (IMAGE_EXTENSIONS, "Imagen", (BLUE, PHOTO)),
        (VIDEO_EXTENSIONS, "Vídeo", (PURPLE, PLAY)),
        (ARCHIVE_EXTENSIONS, "Archivo de imágenes", (ORANGE, ZIPPER)),
    ] {
        for extension in extensions {
            let label = extension.to_ascii_uppercase();
            let icon = match *extension {
                "bmp" | "gif" | "ico" | "jpg" | "png" => {
                    let path = pack.join(format!("{extension}.png"));
                    image::open(&path)
                        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
                        .into_rgba8()
                }
                "mov" | "mp4" | "zip" => {
                    let path = pack.join(format!("{extension}.svg"));
                    let svg = fs::read_to_string(&path)
                        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
                    render_svg(&svg, extension)
                }
                _ => render_svg(
                    &filetype_svg(&label, artwork(extension, default_artwork)),
                    extension,
                ),
            };
            write_icon(output, extension, &icon);
            writeln!(
                associations,
                r#"Root: HKCU; Subkey: "Software\Classes\KGenView.{extension}"; ValueType: string; ValueData: "{family} {label} de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.{extension}\DefaultIcon"; ValueType: string; ValueData: "{{app}}\icons\{extension}.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.{extension}\shell\open\command"; ValueType: string; ValueData: """{{app}}\{{#MyAppExeName}}"" ""%1"""
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".{extension}"; ValueData: "KGenView.{extension}"
Root: HKCU; Subkey: "Software\Classes\.{extension}\OpenWithProgids"; ValueType: none; ValueName: "KGenView.{extension}"; Flags: uninsdeletevalue"#
            )
            .expect("write file association");
        }
    }
    // Keep the shared ProgID used by previous installers usable.
    let legacy_video = render_svg(&filetype_svg("VIDEO", (PURPLE, PLAY)), "video");
    write_icon(output, "video", &legacy_video);
    fs::write(output.join("associations.iss"), associations).expect("write file associations");
}

fn write_icon(output: &Path, name: &str, icon: &RgbaImage) {
    let path = output.join(format!("{name}.ico"));
    fs::write(&path, encode_scaled_icon(icon))
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn render_svg(svg: &str, name: &str) -> RgbaImage {
    const SIZE: u32 = 256;
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
        .unwrap_or_else(|error| panic!("decode {name}.svg: {error}"));
    let size = tree.size();
    assert_eq!(size.width(), size.height(), "{name}.svg must be square");
    let scale = SIZE as f32 / size.width();
    let mut pixmap = tiny_skia::Pixmap::new(SIZE, SIZE).expect("allocate filetype icon");
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut icon = RgbaImage::new(SIZE, SIZE);
    for (target, source) in icon.pixels_mut().zip(pixmap.pixels()) {
        let color = source.demultiply();
        *target = Rgba([color.red(), color.green(), color.blue(), color.alpha()]);
    }
    icon
}

#[derive(Clone, Copy)]
struct Palette {
    ink: &'static str,
    header: &'static str,
    fold: &'static str,
}

const RED: Palette = Palette {
    ink: "#651018",
    header: "#f14754",
    fold: "#a41625",
};
const ORANGE: Palette = Palette {
    ink: "#693104",
    header: "#ffad29",
    fold: "#bc5908",
};
const GREEN: Palette = Palette {
    ink: "#004024",
    header: "#37cb78",
    fold: "#087541",
};
const TEAL: Palette = Palette {
    ink: "#063941",
    header: "#35c3c1",
    fold: "#106e77",
};
const BLUE: Palette = Palette {
    ink: "#102e67",
    header: "#5b9ef3",
    fold: "#1e57ad",
};
const PURPLE: Palette = Palette {
    ink: "#3e0967",
    header: "#b16cf5",
    fold: "#6a26a8",
};
const PINK: Palette = Palette {
    ink: "#68143f",
    header: "#f67cb1",
    fold: "#bd3370",
};

const PHOTO: &str = r#"<circle cx="36" cy="34" r="3"/><path d="M16 53a2 2 0 0 1-1.8-3l6.9-12a2 2 0 0 1 3.5 0l4.3 7.4 3.1-5a2 2 0 0 1 3.5 0l5 9.6a2 2 0 0 1-1.8 3Z"/>"#;
const BITMAP: &str = r#"<path d="M22 36h16v14H22z"/><path d="M16 31h5m-5 0v5m28-5h-5m5 0v5M16 55h5m-5 0v-5m28 5h-5m5 0v-5" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/>"#;
const CAMERA: &str = r##"<path d="M16 38h6l3-4h10l3 4h6a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H16a2 2 0 0 1-2-2V40a2 2 0 0 1 2-2Z"/><circle cx="30" cy="45" r="5" fill="#d7e6ef"/>"##;
const HEIF: &str = r#"<circle cx="30" cy="38" r="8"/><circle cx="24.5" cy="46.5" r="8" fill-opacity=".82"/><circle cx="35.5" cy="46.5" r="8" fill-opacity=".82"/><path d="M27 41h6l2 5-5 4-5-4Z" fill="currentColor"/>"#;
const VECTOR: &str = r#"<path d="M18 50c0-19 24-19 24 0M18 34h24" fill="none" stroke="currentColor" stroke-width="2.4"/><path d="M15 31h6v6h-6zM27 31h6v6h-6zM39 31h6v6h-6zM15 47h6v6h-6zM39 47h6v6h-6z"/>"#;
const CUBE: &str = r#"<path d="m30 30 12 7-12 7-12-7Zm1 16 12-7v11l-12 7Zm-2 0v11l-12-7V39Z"/>"#;
const PLAY: &str =
    r##"<circle cx="30" cy="43" r="13" fill="#d7e6ef"/><path d="m37 43-11 6.5v-13Z"/>"##;
const FILM: &str = r#"<path fill-rule="evenodd" d="M17 32h26a2 2 0 0 1 2 2v18a2 2 0 0 1-2 2H17a2 2 0 0 1-2-2V34a2 2 0 0 1 2-2Zm5 3h-4v4h4Zm0 6h-4v4h4Zm0 6h-4v4h4Zm20-12h-4v4h4Zm0 6h-4v4h4Zm0 6h-4v4h4Zm-7-4-9-5v10Z"/>"#;
const ZIPPER: &str = r#"<path d="M27 31h6m-4 3h4m-6 3h4m-2 3h4m-6 3h4m-1 0v6" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/><path d="M28 48h4l1.2 8h-6.4Z"/>"#;

fn artwork(extension: &str, default: (Palette, &'static str)) -> (Palette, &'static str) {
    match extension {
        "dib" => (RED, BITMAP),
        "jpeg" | "jpe" => (GREEN, PHOTO),
        "tif" | "tiff" => (RED, CAMERA),
        "heic" | "heif" => (PURPLE, HEIF),
        "webp" => (BLUE, PHOTO),
        "jxl" => (PINK, PHOTO),
        "svg" => (PURPLE, VECTOR),
        "dds" => (TEAL, CUBE),
        "hdp" | "jxr" | "wdp" => (BLUE, PHOTO),
        "3g2" | "3gp" | "3gpp" | "f4v" | "flv" => (ORANGE, PLAY),
        "avi" | "divx" | "dv" | "mxf" => (BLUE, FILM),
        "asf" | "wmv" | "wtv" => (TEAL, PLAY),
        "ogm" | "ogv" | "webm" => (GREEN, PLAY),
        "h264" | "h265" | "hevc" | "m1v" | "m2v" | "mpv" | "y4m" => (TEAL, FILM),
        "m2p" | "m2t" | "m2ts" | "mpe" | "mpeg" | "mpg" | "mts" | "ts" | "vob" => (RED, FILM),
        _ => default,
    }
}

fn filetype_svg(label: &str, (palette, symbol): (Palette, &str)) -> String {
    let Palette { ink, header, fold } = palette;
    let scale = (30.0 / (label.len() as f32 * 9.0 - 1.0)).min(1.0);
    let mut lettering = String::new();
    for (index, letter) in label.chars().enumerate() {
        let path = glyph(letter);
        let x = index * 9;
        write!(
            lettering,
            r#"<path d="{path}" transform="translate({x})"/>"#
        )
        .expect("write glyph");
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
<path fill="{ink}" d="M11 3h46v54H11z"/>
<path fill="#f7fcff" d="M53 17v44H7V7h36l10 10z"/>
<path fill="{header}" d="M7 7v18h46v-8L43 7H7z"/>
<path fill="{fold}" d="M53 17h-8a2 2 0 0 1-2-2V7Z"/>
<g transform="translate(12 11) scale({scale} 1)" fill="none" stroke="{ink}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{lettering}</g>
<g fill="{header}" color="{ink}">{symbol}</g>
</svg>"##
    )
}

fn glyph(letter: char) -> &'static str {
    match letter {
        'A' => "M0 10V3a3 3 0 0 1 6 0v7M0 6h6",
        'B' => "M0 10V0h3a2.5 2.5 0 0 1 0 5H0m3 0a2.5 2.5 0 0 1 0 5H0",
        'C' => "M6 1.5A3.2 3.2 0 0 0 0 3v4a3.2 3.2 0 0 0 6 1.5",
        'D' => "M0 10V0h2.5A3.5 3.5 0 0 1 6 3.5v3a3.5 3.5 0 0 1-3.5 3.5Z",
        'E' => "M6 0H0v10h6M0 5h5",
        'F' => "M6 0H0v10M0 5h5",
        'G' => "M6 1.5A3.2 3.2 0 0 0 0 3v4a3 3 0 1 0 6 0V5H3.5",
        'H' => "M0 0v10M6 0v10M0 5h6",
        'I' => "M0 0h6M3 0v10M0 10h6",
        'J' => "M6 0v7a3 3 0 0 1-6 0",
        'K' => "M0 0v10m6-10L0 6m2-2 4 6",
        'L' => "M0 0v10h6",
        'M' => "M0 10V0l3 5 3-5v10",
        'N' => "M0 10V0l6 10V0",
        'O' => "M0 3a3 3 0 0 1 6 0v4a3 3 0 0 1-6 0Z",
        'P' => "M0 10V0h3a2.5 2.5 0 0 1 0 5H0",
        'Q' => "M0 3a3 3 0 0 1 6 0v4a3 3 0 0 1-6 0Zm4 5 3 3",
        'R' => "M0 10V0h3a2.5 2.5 0 0 1 0 5H0m3 0 3 5",
        'S' => "M6 1.2C5-.7 0-.5 0 2.5 0 6 6 4 6 7.5c0 3-4.8 3.4-6 1.2",
        'T' => "M0 0h6M3 0v10",
        'U' => "M0 0v7a3 3 0 1 0 6 0V0",
        'V' => "M0 0v5l3 5 3-5V0",
        'W' => "M0 0l1 10 2-5 2 5L6 0",
        'X' => "M0 0l6 10M6 0 0 10",
        'Y' => "M0 0l3 5 3-5M3 5v5",
        'Z' => "M0 0h6L0 10h6",
        '0' => "M0 3a3 3 0 0 1 6 0v4a3 3 0 0 1-6 0Z",
        '1' => "m0 2 3-2v10M0 10h6",
        '2' => "M0 2a3 3 0 0 1 6 0c0 3-6 4-6 8h6",
        '3' => "M0 1c1.5-1.7 6-1.6 6 1.5C6 4 4 5 2.5 5 4.5 5 6 6 6 7.5c0 3-4.5 3.3-6 1.5",
        '4' => "M0 0v4a2 2 0 0 0 2 2h4m0-6v10",
        '5' => "M6 0H0v5h3a2.5 2.5 0 1 1 0 5H0",
        '6' => "M5.5.8A3.4 3.4 0 0 0 0 3.5V7a3 3 0 1 0 3-3H0",
        '7' => "M0 0h6L2 10",
        '8' => "M3 5a2.5 2.5 0 1 1 0-5 2.5 2.5 0 0 1 0 5 2.5 2.5 0 1 1 0 5 2.5 2.5 0 1 1 0-5",
        '9' => "M.5 9.2A3.4 3.4 0 0 0 6 6.5V3a3 3 0 1 0-3 3h3",
        _ => panic!("unsupported icon glyph: {letter}"),
    }
}
