use bitflags::bitflags;
use vte::ansi::{
    Attr,
    Color, NamedColor,
    CursorStyle, CursorShape,
    LineClearMode, ClearMode, TabulationClearMode,
    Rgb,
};

#[derive(Copy, Clone, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
}

impl Cell {
    pub fn iced_font(&self) -> iced::Font {
        let mut f = iced::Font {
            family: iced::font::Family::name("Drafting* Mono"),
            ..Default::default()
        };
        if self.attrs.contains(CellAttrs::BOLD) {
            f.weight = iced::font::Weight::Bold;
        }
        if self.attrs.contains(CellAttrs::ITALIC) {
            f.style = iced::font::Style::Italic;
        }
        f
    }
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            ch: ' ',
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            attrs: CellAttrs::empty(),
        }
    }
}

bitflags! {
    #[derive(PartialEq, Copy, Clone, Debug)]
    pub struct CellAttrs: u16 {
        const NONE = 0b00;
        const BOLD = 0b01;
        const ITALIC  = 0b10;
    }
}

pub struct Term {
    pub cells: Vec<Vec<Cell>>,
    pub cursor_y: usize,
    pub cursor_x: usize,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
    pub cursor_attrs: CellAttrs,
    pub width: usize,
    pub palette: [Rgb; 16],
}

impl Term {
    pub fn new() -> Term {
        Term {
            cells: Vec::new(),
            cursor_y: 0,
            cursor_x: 0,
            cursor_fg: Color::Named(NamedColor::Foreground),
            cursor_bg: Color::Named(NamedColor::Background),
            cursor_attrs: CellAttrs::empty(),
            width: 80,
            palette: [
                Rgb { r: 0x3c, g: 0x38, b: 0x36 }, //  0 black
                Rgb { r: 0xcc, g: 0x24, b: 0x1d }, //  1 red
                Rgb { r: 0x98, g: 0x97, b: 0x1a }, //  2 green
                Rgb { r: 0xd7, g: 0x99, b: 0x21 }, //  3 yellow
                Rgb { r: 0x45, g: 0x85, b: 0x88 }, //  4 blue
                Rgb { r: 0xb1, g: 0x62, b: 0x86 }, //  5 magenta
                Rgb { r: 0x68, g: 0x9d, b: 0x6a }, //  6 cyan
                Rgb { r: 0xfb, g: 0xeb, b: 0xd7 }, //  7 white
                Rgb { r: 0xc2, g: 0xb3, b: 0xa4 }, //  8 bright black
                Rgb { r: 0x9d, g: 0x00, b: 0x06 }, //  9 bright red
                Rgb { r: 0x79, g: 0x74, b: 0x0e }, // 10 bright green
                Rgb { r: 0xb5, g: 0x76, b: 0x14 }, // 11 bright yellow
                Rgb { r: 0x07, g: 0x66, b: 0x78 }, // 12 bright blue
                Rgb { r: 0x8f, g: 0x3f, b: 0x71 }, // 13 bright magenta
                Rgb { r: 0x42, g: 0x7b, b: 0x58 }, // 14 bright cyan
                Rgb { r: 0x3c, g: 0x38, b: 0x36 }, // 15 bright white
            ],
        }
    }

    pub fn resolve(&self, color: Color) -> iced::Color {
        let rgb = match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(ind) => self.palette[ind as usize],
            Color::Named(NamedColor::Black) => self.palette[0],
            Color::Named(NamedColor::Red) => self.palette[1],
            Color::Named(NamedColor::Green) => self.palette[2],
            Color::Named(NamedColor::Yellow) => self.palette[3],
            Color::Named(NamedColor::Blue) => self.palette[4],
            Color::Named(NamedColor::Magenta) => self.palette[5],
            Color::Named(NamedColor::Cyan) => self.palette[6],
            Color::Named(NamedColor::White) => self.palette[7],
            Color::Named(NamedColor::BrightBlack) => self.palette[8],
            Color::Named(NamedColor::BrightRed) => self.palette[9],
            Color::Named(NamedColor::BrightGreen) => self.palette[10],
            Color::Named(NamedColor::BrightYellow) => self.palette[11],
            Color::Named(NamedColor::BrightBlue) => self.palette[12],
            Color::Named(NamedColor::BrightMagenta) => self.palette[13],
            Color::Named(NamedColor::BrightCyan) => self.palette[14],
            Color::Named(NamedColor::BrightWhite) => self.palette[15],
            Color::Named(NamedColor::Foreground) => self.palette[0],
            Color::Named(NamedColor::Background) => self.palette[7],
            Color::Named(NamedColor::Cursor) => self.palette[0],
            Color::Named(NamedColor::DimBlack) => self.palette[0] * 0.67,
            Color::Named(NamedColor::DimRed) => self.palette[1] * 0.67,
            Color::Named(NamedColor::DimGreen) => self.palette[2] * 0.67,
            Color::Named(NamedColor::DimYellow) => self.palette[3] * 0.67,
            Color::Named(NamedColor::DimBlue) => self.palette[4] * 0.67,
            Color::Named(NamedColor::DimMagenta) => self.palette[5] * 0.67,
            Color::Named(NamedColor::DimCyan) => self.palette[6] * 0.67,
            Color::Named(NamedColor::DimWhite) => self.palette[7] * 0.67,
            Color::Named(NamedColor::BrightForeground) => self.palette[8],
            Color::Named(NamedColor::DimForeground) => self.palette[0] * 0.67,
        };

        iced::Color::from_rgb8(rgb.r, rgb.g, rgb.b)
    }
}

impl vte::ansi::Handler for Term {
    /// OSC to set window title.
    fn set_title(&mut self, _: Option<String>) {}

    /// Set the cursor style.
    fn set_cursor_style(&mut self, _: Option<CursorStyle>) {}

    /// Set the cursor shape.
    fn set_cursor_shape(&mut self, _shape: CursorShape) {}

    /// A character to be displayed.
    fn input(&mut self, ch: char) {
        while self.cells.len() <= self.cursor_y {
            self.cells.push(vec![
                Cell::default();
                self.width
            ]);
        }

        self.cells[self.cursor_y][self.cursor_x] = Cell {
            ch,
            fg: self.cursor_fg,
            bg: self.cursor_bg,
            attrs: self.cursor_attrs,
        };
        self.cursor_x += 1;

        if self.cursor_x >= self.width {
            self.cursor_x = 0;
            self.cursor_y += 1;
        }
    }

    /// Set cursor to position.
    fn goto(&mut self, _line: i32, _col: usize) {}

    /// Set cursor to specific row.
    fn goto_line(&mut self, _line: i32) {}

    /// Set cursor to specific column.
    fn goto_col(&mut self, _col: usize) {}

    /// Insert blank characters in current line starting from cursor.
    fn insert_blank(&mut self, _: usize) {}

    /// Move cursor up `rows`.
    fn move_up(&mut self, _: usize) {}

    /// Move cursor down `rows`.
    fn move_down(&mut self, _: usize) {}

    /// Identify the terminal (should write back to the pty stream).
    fn identify_terminal(&mut self, _intermediate: Option<char>) {}

    /// Report device status.
    fn device_status(&mut self, _: usize) {}

    /// Move cursor forward `cols`.
    fn move_forward(&mut self, _col: usize) {}

    /// Move cursor backward `cols`.
    fn move_backward(&mut self, _col: usize) {}

    /// Move cursor down `rows` and set to column 1.
    fn move_down_and_cr(&mut self, _row: usize) {}

    /// Move cursor up `rows` and set to column 1.
    fn move_up_and_cr(&mut self, _row: usize) {}

    /// Put `count` tabs.
    fn put_tab(&mut self, mut count: u16) {
        while self.cursor_x < self.width && count > 0 {
            count -= 1;

            if self.cells[self.cursor_y][self.cursor_x].ch == ' ' {
                self.cells[self.cursor_y][self.cursor_x].ch = '\t';
            }

            loop {
                if self.cursor_x + 1 == self.width {
                    break;
                }

                self.cursor_x += 1;

                if self.cursor_x & 7 == 0 {
                    break;
                }
            }
        }
    }

    /// Backspace `count` characters.
    fn backspace(&mut self) {}

    /// Carriage return.
    fn carriage_return(&mut self) {
        self.cursor_x = 0;
    }

    /// Linefeed.
    fn linefeed(&mut self) {
        self.cursor_y += 1;
    }

    /// Ring the bell.
    ///
    /// Hopefully this is never implemented.
    fn bell(&mut self) {}

    /// Substitute char under cursor.
    fn substitute(&mut self) {}

    /// Newline.
    fn newline(&mut self) {
        self.cursor_y += 1;
    }

    /// Set current position as a tabstop.
    fn set_horizontal_tabstop(&mut self) {}

    /// Scroll up `rows` rows.
    fn scroll_up(&mut self, _: usize) {}

    /// Scroll down `rows` rows.
    fn scroll_down(&mut self, _: usize) {}

    /// Insert `count` blank lines.
    fn insert_blank_lines(&mut self, _: usize) {}

    /// Delete `count` lines.
    fn delete_lines(&mut self, _: usize) {}

    /// Erase `count` chars in current line following cursor.
    ///
    /// Erase means resetting to the default state (default colors, no content,
    /// no mode flags).
    fn erase_chars(&mut self, _: usize) {}

    /// Delete `count` chars.
    ///
    /// Deleting a character is like the delete key on the keyboard - everything
    /// to the right of the deleted things is shifted left.
    fn delete_chars(&mut self, _: usize) {}

    /// Move backward `count` tabs.
    fn move_backward_tabs(&mut self, _count: u16) { }

    /// Move forward `count` tabs.
    fn move_forward_tabs(&mut self, count: u16) {
        for _ in 0..count {
            self.cursor_x = (self.cursor_x + 7) & !7;
        }
        self.cursor_x = self.cursor_x.max(self.width - 1);
    }

    /// Save current cursor position.
    fn save_cursor_position(&mut self) {}

    /// Restore cursor position.
    fn restore_cursor_position(&mut self) {}

    /// Clear current line.
    fn clear_line(&mut self, _mode: LineClearMode) {}

    /// Clear screen.
    fn clear_screen(&mut self, _mode: ClearMode) {}

    /// Clear tab stops.
    fn clear_tabs(&mut self, _mode: TabulationClearMode) {}

    /// Set tab stops at every `interval`.
    fn set_tabs(&mut self, _interval: u16) {}

    /// Reset terminal state.
    fn reset_state(&mut self) {}

    /// Reverse Index.
    ///
    /// Move the active position to the same horizontal position on the
    /// preceding line. If the active position is at the top margin, a scroll
    /// down is performed.
    fn reverse_index(&mut self) {}

    fn terminal_attribute(&mut self, attr: Attr) {
        match attr {
            Attr::Foreground(color) => self.cursor_fg = color,
            Attr::Background(color) => self.cursor_bg = color,
            Attr::Reset => {
                self.cursor_fg = Color::Named(NamedColor::Foreground);
                self.cursor_bg = Color::Named(NamedColor::Background);
                self.cursor_attrs = CellAttrs::empty();
            },
            Attr::Bold => self.cursor_attrs.insert(CellAttrs::BOLD),
            Attr::CancelBold => self.cursor_attrs.remove(CellAttrs::BOLD),
            Attr::Italic => self.cursor_attrs.insert(CellAttrs::ITALIC),
            Attr::CancelItalic => self.cursor_attrs.remove(CellAttrs::ITALIC),
            _ => (),
        }
    }
}

