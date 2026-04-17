use crate::styles::{Theme, TextClass};

use iced::Font;
use iced::font;
use iced::advanced::text::IntoFragment;
use iced::widget::{text, Text};

pub fn thead<'a>(f: impl IntoFragment<'a>) -> Text<'a, Theme> {
    text(f)
        .font(Font {
            weight: font::Weight::Bold,
            ..Default::default()
        })
        .size(14.5)
}

// pub fn bold<'a>(f: impl IntoFragment<'a>) -> Text<'a, Theme> {
//     text(f)
//         .font(Font {
//             weight: font::Weight::Bold,
//             ..Default::default()
//         })
// }

pub fn mono<'a>(f: impl IntoFragment<'a>) -> Text<'a, Theme> {
    text(f)
        .font(Font {
            family: font::Family::name("Drafting* Mono"),
            ..Default::default()
        })
        .size(15.)
}
