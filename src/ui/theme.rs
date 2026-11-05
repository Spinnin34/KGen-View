use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

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
