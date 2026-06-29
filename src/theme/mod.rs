use slint::{Color, Global};

use crate::ui::{Colors, MainWindow};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    pub shades: PaletteShades,
}

#[derive(Debug, Clone, Copy)]
pub struct PaletteShades {
    pub s2: Color,
    pub s5: Color,
    pub s10: Color,
    pub s20: Color,
    pub s30: Color,
    pub s40: Color,
    pub s50: Color,
    pub s60: Color,
    pub s70: Color,
    pub s80: Color,
    pub s90: Color,
}

const fn hex(value: u32) -> Color {
    Color::from_rgb_u8(
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    )
}

const fn lerp_u8(from: u8, to: u8, amount: u8) -> u8 {
    let from = from as u16;
    let to = to as u16;
    let amount = amount as u16;
    ((from * (255 - amount) + to * amount) / 255) as u8
}

const fn darken(r: u8, g: u8, b: u8, amount: u8) -> Color {
    Color::from_rgb_u8(lerp_u8(r, 0, amount), lerp_u8(g, 0, amount), lerp_u8(b, 0, amount))
}

const fn lighten(r: u8, g: u8, b: u8, amount: u8) -> Color {
    Color::from_rgb_u8(
        lerp_u8(r, 255, amount),
        lerp_u8(g, 255, amount),
        lerp_u8(b, 255, amount),
    )
}

/// Builds a CRT-style monochrome ramp with `base` at the 50 swatch.
const fn palette_from_base(name: &'static str, r: u8, g: u8, b: u8) -> Palette {
    Palette {
        name,
        shades: PaletteShades {
            s2: darken(r, g, b, 242),
            s5: darken(r, g, b, 224),
            s10: darken(r, g, b, 199),
            s20: darken(r, g, b, 158),
            s30: darken(r, g, b, 115),
            s40: darken(r, g, b, 64),
            s50: Color::from_rgb_u8(r, g, b),
            s60: lighten(r, g, b, 64),
            s70: lighten(r, g, b, 115),
            s80: lighten(r, g, b, 166),
            s90: lighten(r, g, b, 217),
        },
    }
}

pub const TEAL: Palette = Palette {
    name: "teal",
    shades: PaletteShades {
        s2: hex(0x061010),
        s5: hex(0x091E1D),
        s10: hex(0x0D3531),
        s20: hex(0x166259),
        s30: hex(0x1F8C7F),
        s40: hex(0x27B4A2),
        s50: hex(0x2DD4BF),
        s60: hex(0x66E1D1),
        s70: hex(0x94ECDF),
        s80: hex(0xBEF6EC),
        s90: hex(0xE6FFF9),
    },
};

pub const GOLD: Palette = palette_from_base("gold", 0xFD, 0xCB, 0x6E);
pub const GREEN: Palette = palette_from_base("green", 0x2E, 0xD5, 0x73);
pub const GREY: Palette = palette_from_base("grey", 0x95, 0xA5, 0xA6);
pub const RED: Palette = palette_from_base("red", 0xFF, 0x6B, 0x81);
pub const PURPLE: Palette = palette_from_base("purple", 0xA2, 0x9B, 0xFE);
pub const BLUE: Palette = palette_from_base("blue", 0x74, 0xB9, 0xFF);

pub const PALETTES: &[Palette] = &[
    TEAL, GOLD, GREEN, GREY, RED, PURPLE, BLUE,
];

pub fn apply_palette(window: &MainWindow, palette: &Palette) {
    let colors = Colors::get(window);
    let shades = palette.shades;

    colors.set_primary_2(shades.s2);
    colors.set_primary_5(shades.s5);
    colors.set_primary_10(shades.s10);
    colors.set_primary_20(shades.s20);
    colors.set_primary_30(shades.s30);
    colors.set_primary_40(shades.s40);
    colors.set_primary_50(shades.s50);
    colors.set_primary_60(shades.s60);
    colors.set_primary_70(shades.s70);
    colors.set_primary_80(shades.s80);
    colors.set_primary_90(shades.s90);
}

pub fn apply_palette_by_index(window: &MainWindow, index: usize) {
    let palette = &PALETTES[index % PALETTES.len()];
    apply_palette(window, palette);
}

pub fn next_palette_index(current: usize) -> usize {
    (current + 1) % PALETTES.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_are_non_empty() {
        assert!(PALETTES.len() >= 2);
    }

    #[test]
    fn palette_names_are_unique() {
        let mut names = PALETTES.iter().map(|palette| palette.name).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PALETTES.len());
    }

    #[test]
    fn generated_palette_uses_base_at_s50() {
        let gold = GOLD.shades.s50;
        assert_eq!(gold.red(), 0xFD);
        assert_eq!(gold.green(), 0xCB);
        assert_eq!(gold.blue(), 0x6E);
    }

    #[test]
    fn generated_palette_darkens_and_lightens() {
        let purple = &PURPLE.shades;
        assert!(purple.s10.red() < purple.s50.red());
        assert!(purple.s90.red() > purple.s50.red());
    }
}
