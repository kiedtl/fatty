use crate::styles::{Theme, TextClass};

use iced::Font;
use iced::font;
use iced::advanced::text::IntoFragment;
use iced::widget::{text, Text, Column};

#[macro_export]
macro_rules! mycolumn {
    (@build $col:expr; if let $pat:pat = $val:expr => $item:expr, $($rest:tt)*) => { // if let p = v => item (more items later)
        mycolumn!(@build (if let $pat = $val { $col.push($item) } else { $col }); $($rest)*)
    };
    (@build $col:expr; if let $pat:pat = $val:expr => $item:expr) => { // if let p = v => item (last item)
        if let $pat = $val { $col.push($item) } else { $col }
    };
    (@build $col:expr; if $cond:expr => $item:expr, $($rest:tt)*) => { // if cond => item (more items later)
        mycolumn!(@build (if $cond { $col.push($item) } else { $col }); $($rest)*)
    };
    (@build $col:expr; if $cond:expr => $item:expr) => { // if cond => item (last item)
        if $cond { $col.push($item) } else { $col }
    };
    (@build $col:expr; $item:expr, $($rest:tt)*) => { // plain item (more items later)
        mycolumn!(@build $col.push($item); $($rest)*)
    };
    (@build $col:expr; $item:expr) => { // last item
        $col.push($item)
    };
    (@build $col:expr; ) => { $col }; // nothing left
    ($($tail:tt)*) => {{ // entry point
        mycolumn!(@build Column::new(); $($tail)*)
    }};
}

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
