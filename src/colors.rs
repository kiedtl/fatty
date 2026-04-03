#[derive(Copy, Clone, Debug)]
pub struct Hsv {
    pub h: u16,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub const fn from_rgb(rgb: u32) -> Hsv {
        let (rf, gf, bf) = deconstruct_rgb_to_f32(rgb);
        let cmin = rf.min(gf).min(bf);
        let cmax = rf.max(gf).max(bf);
        let diff = cmax - cmin;
        let h = 60 *
            if rf >= gf && rf >= bf {
                ((gf - bf) / diff) % 6.
            } else if gf >= bf {
                ((bf - rf) / diff) + 2.
            } else {
                ((rf - gf) / diff) + 4.
            } as u16;
        let s = if cmax < f32::EPSILON { 0. } else { diff / cmax };
        Hsv { h, s, v: cmax }
    }

    pub const fn to_rgb(self) -> (u8, u8, u8) {
        let c = self.v * self.s;
        let x = c * (1. - (self.h as f32 / 60. % 2. - 1.).abs());
        let m = self.v - c;
        let (rf, gf, bf) = match self.h {
               0..60 => (c, x, 0.),
             60..120 => (x, c, 0.),
            120..180 => (0., c, x),
            180..240 => (0., x, c),
            240..300 => (x, 0., c),
            300..360 => (c, 0., x),
            _ => (0., 0., 0.)
        };
        (
            ((rf + m) * 255.) as u8,
            ((gf + m) * 255.) as u8,
            ((bf + m) * 255.) as u8,
        )
    }

    pub const fn to_color(self) -> iced::Color {
        let (r, g, b) = self.to_rgb();
        let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        iced::Color { r, g, b, a: 1. }
    }

    pub const fn value(mut self, value: f32) -> Hsv {
        self.v = value;
        self
    }
}

pub const fn hsv(rgb: u32) -> Hsv {
    Hsv::from_rgb(rgb)
}

pub const fn of_lightness(rgb: u32, lightness: f32) -> u32 {
    let (r, g, b) = hsv(rgb).value(lightness).to_rgb(); 
    construct_rgb(r, g, b)
}

pub const fn deconstruct_rgb(rgb: u32) -> (u8, u8, u8) {
    (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

pub const fn deconstruct_rgb_to_f32(rgb: u32) -> (f32, f32, f32) {
    let (r, g, b) = deconstruct_rgb(rgb);
    (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

pub const fn construct_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const fn iced_color(rgb: u32) -> iced::Color {
    let (r, g, b) = deconstruct_rgb_to_f32(rgb);
    iced::Color { r, g, b, a: 1. }
}
