#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(Copy, Clone, Debug)]
pub struct CIELAB {
    pub L: f64,
    pub a: f64,
    pub b: f64
}

impl CIELAB {
    pub fn from_rgb(rgb: iced::Color) -> CIELAB {
        let lin = |c: f32| {
            let c = c as f64;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let f = |t: f64| if t > 0.0088564517 { t.powf(1. / 3.) } else { 7.787037 * t + 16. / 116. };

        let r = lin(rgb.r);
        let g = lin(rgb.g);
        let b = lin(rgb.b);

        let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
        let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
        let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;

        let fx = f(x);
        let fy = f(y);
        let fz = f(z);

        CIELAB {
            L: 116. * fy - 16.,
            a: 500. * (fx - fy),
            b: 200. * (fy - fz),
        }
    }

    pub fn to_rgb(&self) -> iced::Color {
        let f_inv = |t: f64|
            if t.powi(3) > 0.0088564517 {
                t.powi(3)
            } else {
                (t - 16.0 / 116.0) / 7.787037
            };

        let gamma = |c: f64|
            (if c <= 0.0031308 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }).clamp(0., 1.) as f32;

        let fy = (self.L + 16.) / 116.;
        let fx = fy + self.a / 500.;
        let fz = fy - self.b / 200.;

        let x = 0.95047 * f_inv(fx);
        let y = f_inv(fy);
        let z = 1.08883 * f_inv(fz);

        let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
        let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
        let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;

        iced::Color::from_rgb(gamma(r), gamma(g), gamma(b))
    }

    // https://gist.github.com/jake-stewart/0a8ea46159a7da2c808e5be2177e1783
    pub fn lerp(&self, other: CIELAB, t: f64) -> CIELAB {
        CIELAB {
            L: self.L + t * (other.L - self.L),
            a: self.a + t * (other.a - self.a),
            b: self.b + t * (other.b - self.b),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Hsv {
    pub h: u16,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    // pub const fn from_rgb(rgb: u32) -> Hsv {
    //     let (rf, gf, bf) = deconstruct_rgb_to_f32(rgb);
    //     let cmin = rf.min(gf).min(bf);
    //     let cmax = rf.max(gf).max(bf);
    //     let diff = cmax - cmin;
    //     let h = 60 *
    //         if rf >= gf && rf >= bf {
    //             ((gf - bf) / diff) % 6.
    //         } else if gf >= bf {
    //             ((bf - rf) / diff) + 2.
    //         } else {
    //             ((rf - gf) / diff) + 4.
    //         } as u16;
    //     let s = if cmax < f32::EPSILON { 0. } else { diff / cmax };
    //     Hsv { h, s, v: cmax }
    // }

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

    // pub const fn value(mut self, value: f32) -> Hsv {
    //     self.v = value;
    //     self
    // }
}

// pub const fn hsv(rgb: u32) -> Hsv {
//     Hsv::from_rgb(rgb)
// }

// pub const fn of_lightness(rgb: u32, lightness: f32) -> u32 {
//     let (r, g, b) = hsv(rgb).value(lightness).to_rgb(); 
//     construct_rgb(r, g, b)
// }

pub const fn decompose(rgb: u32) -> (u8, u8, u8) {
    (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8)
}

pub const fn deconstruct_rgb_to_f32(rgb: u32) -> (f32, f32, f32) {
    let (r, g, b) = decompose(rgb);
    (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

pub const fn compose(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const fn iced_color(rgb: u32) -> iced::Color {
    let (r, g, b) = deconstruct_rgb_to_f32(rgb);
    iced::Color { r, g, b, a: 1. }
}

fn interpolate(a: u8, b: u8, f: f64) -> u8 {
    let aa = a as f64 / 255.;
    let ab = b as f64 / 255.;
    ((aa + f * (ab - aa)) * 255.).min(255.) as u8
}

pub fn mix(a: u32, b: u32, frac: f64) -> u32 {
    let (ar, ag, ab) = decompose(a);
    let (br, bg, bb) = decompose(b);
    let rr = interpolate(ar, br, frac);
    let rg = interpolate(ag, bg, frac);
    let rb = interpolate(ab, bb, frac);
    compose(rr, rg, rb)
}
