use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub struct MenuPalette {
    pub surface: u32,
    pub border: u32,
    pub hover: u32,
    pub text: u32,
    pub muted: u32,
    pub selected: u32,
    pub selected_text: u32,
    pub accent: u32,
}

impl MenuPalette {
    pub const fn for_dark(dark: bool) -> Self {
        if dark {
            Self {
                surface: 0x202024,
                border: 0x3d3d42,
                hover: 0x36363d,
                text: 0xf4f4f4,
                muted: 0xb4b4b8,
                selected: 0x342a26,
                selected_text: 0xffbb89,
                accent: 0xff8d42,
            }
        } else {
            Self {
                surface: 0xfafafb,
                border: 0xd2d6de,
                hover: 0xeaebef,
                text: 0x212127,
                muted: 0x565660,
                selected: 0xf9e9df,
                selected_text: 0x8c3815,
                accent: 0xa54214,
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: D2D1_COLOR_F,
    pub chrome: D2D1_COLOR_F,
    pub raised: D2D1_COLOR_F,
    pub hover: D2D1_COLOR_F,
    pub divider: D2D1_COLOR_F,
    pub accent: D2D1_COLOR_F,
}

impl Palette {
    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self {
                canvas: rgb(0x202020),
                chrome: rgb(0x181818),
                raised: rgb(0x242424),
                hover: rgb(0x303030),
                divider: rgb(0x353535),
                accent: rgb(0xFF8A2A),
            }
        } else {
            Self {
                canvas: rgb(0xF2F3F5),
                chrome: rgb(0xFAFAFB),
                raised: rgb(0xEEF0F3),
                hover: rgb(0xE1E4EA),
                divider: rgb(0xD2D6DE),
                accent: rgb(0xEA6716),
            }
        }
    }
}

pub const fn rgb(value: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((value >> 16) & 0xff) as f32 / 255.0,
        g: ((value >> 8) & 0xff) as f32 / 255.0,
        b: (value & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}
