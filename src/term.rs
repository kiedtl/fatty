use bitflags::bitflags;
use vte::ansi::{
    Attr,
    Color, NamedColor,
    CursorStyle, CursorShape,
    LineClearMode, ClearMode, TabulationClearMode,
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
        }
    }

    pub fn resolve(&self, theme: &crate::styles::Theme, color: Color) -> iced::Color {
        let rgb = match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(ind) => theme.palette[ind as usize],
            Color::Named(NamedColor::Black) => theme.palette[0],
            Color::Named(NamedColor::Red) => theme.palette[1],
            Color::Named(NamedColor::Green) => theme.palette[2],
            Color::Named(NamedColor::Yellow) => theme.palette[3],
            Color::Named(NamedColor::Blue) => theme.palette[4],
            Color::Named(NamedColor::Magenta) => theme.palette[5],
            Color::Named(NamedColor::Cyan) => theme.palette[6],
            Color::Named(NamedColor::White) => theme.palette[7],
            Color::Named(NamedColor::BrightBlack) => theme.palette[8],
            Color::Named(NamedColor::BrightRed) => theme.palette[9],
            Color::Named(NamedColor::BrightGreen) => theme.palette[10],
            Color::Named(NamedColor::BrightYellow) => theme.palette[11],
            Color::Named(NamedColor::BrightBlue) => theme.palette[12],
            Color::Named(NamedColor::BrightMagenta) => theme.palette[13],
            Color::Named(NamedColor::BrightCyan) => theme.palette[14],
            Color::Named(NamedColor::BrightWhite) => theme.palette[15],
            Color::Named(NamedColor::Foreground) => theme.palette[0],
            Color::Named(NamedColor::Background) => theme.palette[7],
            Color::Named(NamedColor::Cursor) => theme.palette[0],
            Color::Named(NamedColor::DimBlack) => theme.palette[0] * 0.67,
            Color::Named(NamedColor::DimRed) => theme.palette[1] * 0.67,
            Color::Named(NamedColor::DimGreen) => theme.palette[2] * 0.67,
            Color::Named(NamedColor::DimYellow) => theme.palette[3] * 0.67,
            Color::Named(NamedColor::DimBlue) => theme.palette[4] * 0.67,
            Color::Named(NamedColor::DimMagenta) => theme.palette[5] * 0.67,
            Color::Named(NamedColor::DimCyan) => theme.palette[6] * 0.67,
            Color::Named(NamedColor::DimWhite) => theme.palette[7] * 0.67,
            Color::Named(NamedColor::BrightForeground) => theme.palette[8],
            Color::Named(NamedColor::DimForeground) => theme.palette[0] * 0.67,
        };

        iced::Color::from_rgb8(rgb.r, rgb.g, rgb.b)
    }

    pub fn allocate_rows_until(&mut self, y: usize) {
        while self.cells.len() <= y {
            self.cells.push(vec![Cell::default(); self.width]);
        }
    }

    pub fn cursor_cell(&mut self) -> Cell {
        self.allocate_rows_until(self.cursor_y);
        self.cells[self.cursor_y][self.cursor_x]
    }

    pub fn cursor_cell_mut(&mut self) -> &mut Cell {
        self.allocate_rows_until(self.cursor_y);
        &mut self.cells[self.cursor_y][self.cursor_x]
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
        self.allocate_rows_until(self.cursor_y);

        *self.cursor_cell_mut() = Cell {
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

            if self.cursor_cell().ch == ' ' {
                self.cursor_cell_mut().ch = '\t';
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

