//! Text inputs display fields that can be filled with text. Copied from Iced.
use unicode_segmentation::UnicodeSegmentation;
use std::time::{Instant, Duration};

use iced::{
    alignment,
    keyboard::{self, key, Modifiers},
    advanced::{
        clipboard::{self, Clipboard},
        layout, renderer,
        mouse::{self, click},
        input_method,
        text::{
            self, Text,
            paragraph::{self, Paragraph as _},
        },
        widget::{
            self,
            operation::{self, Operation},
            tree::{self, Tree},
        },
        InputMethod, Layout, Shell, Widget,
    },
    touch,
    window,
    Alignment, Background, Border, Color, Element, Event, Length, Padding,
    Pixels, Point, Rectangle, Size, Vector,
};

use crate::styles::Theme as MyTheme;
use crate::ControlMode;

struct Annotation {
    s: usize,
    e: usize,
}

pub fn input<'a, Message, Theme, Renderer>(
    mode: ControlMode,
    placeholder: &str,
    value: &str,
) -> TextInput<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog + 'a,
    Renderer: text::Renderer,
{
    TextInput::new(mode, placeholder, value)
}

/// A field that can be filled with text.
pub struct TextInput<'a, Message, Theme = MyTheme, Renderer = iced::Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    mode: ControlMode,
    id: Option<widget::Id>,
    placeholder: String,
    value: Value,
    font: Option<Renderer::Font>,
    width: Length,
    padding: Padding,
    size: Option<Pixels>,
    line_height: text::LineHeight,
    alignment: alignment::Horizontal,
    on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_paste: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_submit: Option<Message>,
    icon: Option<Icon<Renderer::Font>>,
    class: Theme::Class<'a>,
    last_status: Option<Status>,
    annotations: Vec<Annotation>,
}

/// The default [`Padding`] of a [`TextInput`].
pub const DEFAULT_PADDING: Padding = Padding::new(5.0);

impl<'a, Message, Theme, Renderer> TextInput<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// Creates a new [`TextInput`] with the given placeholder and
    /// its current value.
    pub fn new(mode: ControlMode, placeholder: &str, value: &str) -> Self {
        TextInput {
            mode,
            id: None,
            placeholder: String::from(placeholder),
            value: Value::new(value),
            font: None,
            width: Length::Fill,
            padding: DEFAULT_PADDING,
            size: None,
            line_height: text::LineHeight::default(),
            alignment: alignment::Horizontal::Left,
            on_input: None,
            on_paste: None,
            on_submit: None,
            icon: None,
            class: Theme::default(),
            last_status: None,
            annotations: Vec::new(),
        }
    }

    /// Sets the [`widget::Id`] of the [`TextInput`].
    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the message that should be produced when some text is typed into
    /// the [`TextInput`].
    ///
    /// If this method is not called, the [`TextInput`] will be disabled.
    pub fn on_input(mut self, on_input: impl Fn(String) -> Message + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// Sets the message that should be produced when some text is typed into
    /// the [`TextInput`], if `Some`.
    ///
    /// If `None`, the [`TextInput`] will be disabled.
    pub fn on_input_maybe(mut self, on_input: Option<impl Fn(String) -> Message + 'a>) -> Self {
        self.on_input = on_input.map(|f| Box::new(f) as _);
        self
    }

    /// Sets the message that should be produced when the [`TextInput`] is
    /// focused and the enter key is pressed.
    pub fn on_submit(mut self, message: Message) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// Sets the message that should be produced when the [`TextInput`] is
    /// focused and the enter key is pressed, if `Some`.
    pub fn on_submit_maybe(mut self, on_submit: Option<Message>) -> Self {
        self.on_submit = on_submit;
        self
    }

    /// Sets the message that should be produced when some text is pasted into
    /// the [`TextInput`].
    pub fn on_paste(mut self, on_paste: impl Fn(String) -> Message + 'a) -> Self {
        self.on_paste = Some(Box::new(on_paste));
        self
    }

    /// Sets the message that should be produced when some text is pasted into
    /// the [`TextInput`], if `Some`.
    pub fn on_paste_maybe(mut self, on_paste: Option<impl Fn(String) -> Message + 'a>) -> Self {
        self.on_paste = on_paste.map(|f| Box::new(f) as _);
        self
    }

    /// Sets the [`Font`] of the [`TextInput`].
    ///
    /// [`Font`]: text::Renderer::Font
    pub fn font(mut self, font: Renderer::Font) -> Self {
        self.font = Some(font);
        self
    }

    /// Sets the [`Icon`] of the [`TextInput`].
    pub fn icon(mut self, icon: Icon<Renderer::Font>) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets the width of the [`TextInput`].
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the [`Padding`] of the [`TextInput`].
    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the text size of the [`TextInput`].
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    /// Sets the [`text::LineHeight`] of the [`TextInput`].
    pub fn line_height(mut self, line_height: impl Into<text::LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    /// Sets the horizontal alignment of the [`TextInput`].
    pub fn align_x(mut self, alignment: impl Into<alignment::Horizontal>) -> Self {
        self.alignment = alignment.into();
        self
    }

    /// Sets the style of the [`TextInput`].
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the style class of the [`TextInput`].
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    pub fn add_annotation(&mut self, s: usize, e: usize) {
        self.annotations.push(Annotation { s, e });
    }

    /// Lays out the [`TextInput`], overriding its [`Value`] if provided.
    ///
    /// [`Renderer`]: text::Renderer
    pub fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
        value: Option<&Value>,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let value = value.unwrap_or(&self.value);

        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let text_size = self.size.unwrap_or_else(|| renderer.default_size());
        let padding = self.padding.fit(Size::ZERO, limits.max());
        let height = self.line_height.to_absolute(text_size);

        let limits = limits.width(self.width).shrink(padding);
        let text_bounds = limits.resolve(self.width, height, Size::ZERO);

        let placeholder_text = Text {
            font,
            line_height: self.line_height,
            content: self.placeholder.as_str(),
            bounds: Size::new(f32::INFINITY, text_bounds.height),
            size: text_size,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::default(),
            hint_factor: renderer.scale_factor(),
            ellipsis: Default::default(),
        };

        let _ = state.placeholder.update(placeholder_text);

        let _ = state.value.update(Text {
            content: &value.to_string(),
            ..placeholder_text
        });

        if let Some(icon) = &self.icon {
            let mut content = [0; 4];

            let icon_text = Text {
                line_height: self.line_height,
                content: icon.code_point.encode_utf8(&mut content) as &_,
                font: icon.font,
                size: icon.size.unwrap_or_else(|| renderer.default_size()),
                bounds: Size::new(f32::INFINITY, text_bounds.height),
                align_x: text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::default(),
                hint_factor: renderer.scale_factor(),
                ellipsis: Default::default(),
            };

            let _ = state.icon.update(icon_text);

            let icon_width = state.icon.min_width();

            let (text_position, icon_position) = match icon.side {
                Side::Left => (
                    Point::new(padding.left + icon_width + icon.spacing, padding.top),
                    Point::new(padding.left, padding.top),
                ),
                Side::Right => (
                    Point::new(padding.left, padding.top),
                    Point::new(padding.left + text_bounds.width - icon_width, padding.top),
                ),
            };

            let text_node =
                layout::Node::new(text_bounds - Size::new(icon_width + icon.spacing, 0.0))
                    .move_to(text_position);

            let icon_node =
                layout::Node::new(Size::new(icon_width, text_bounds.height)).move_to(icon_position);

            layout::Node::with_children(text_bounds.expand(padding), vec![text_node, icon_node])
        } else {
            let text =
                layout::Node::new(text_bounds).move_to(Point::new(padding.left, padding.top));

            layout::Node::with_children(text_bounds.expand(padding), vec![text])
        }
    }

    // fn input_method<'b>(
    //     &self,
    //     state: &'b State<Renderer::Paragraph>,
    //     layout: Layout<'_>,
    //     value: &Value,
    // ) -> InputMethod<&'b str> {
    //     let Some(Focus {
    //         is_window_focused: true,
    //         ..
    //     }) = &state.is_focused
    //     else {
    //         return InputMethod::Disabled;
    //     };

    //     let text_bounds = layout.children().next().unwrap().bounds();

    //     let caret_index = match state.cursor.state(value) {
    //         CursorState::Index(position) => position,
    //         CursorState::Selection { start, end } => start.min(end),
    //     };

    //     let text = state.value.raw();
    //     let (cursor_x, scroll_offset) =
    //         measure_cursor_and_scroll_offset(text, text_bounds, caret_index);

    //     let alignment_offset =
    //         alignment_offset(text_bounds.width, text.min_width(), self.alignment);

    //     let x = (text_bounds.x + cursor_x).floor() - scroll_offset + alignment_offset;

    //     InputMethod::Enabled {
    //         cursor: Rectangle::new(
    //             Point::new(x, text_bounds.y),
    //             Size::new(1.0, text_bounds.height),
    //         ),
    //         purpose: input_method::Purpose::Normal,
    //         preedit: state.preedit.as_ref().map(input_method::Preedit::as_ref),
    //     }
    // }

    /// Draws the [`TextInput`] with the given [`Renderer`], overriding its
    /// [`Value`] if provided.
    ///
    /// [`Renderer`]: text::Renderer
    pub fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        value: Option<&Value>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let value = value.unwrap_or(&self.value);
        let is_disabled = self.on_input.is_none();

        let bounds = layout.bounds();

        let mut children_layout = layout.children();
        let text_bounds = children_layout.next().unwrap().bounds();

        let style = theme.style(&self.class, self.last_status.unwrap_or(Status::Disabled));

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: style.border,
                ..renderer::Quad::default()
            },
            style.background,
        );

        if self.icon.is_some() {
            let icon_layout = children_layout.next().unwrap();

            let icon = state.icon.raw();

            renderer.fill_paragraph(
                icon,
                icon_layout.bounds().anchor(
                    icon.min_bounds(),
                    Alignment::Center,
                    Alignment::Center,
                ),
                style.icon,
                *viewport,
            );
        }

        let text = value.to_string();

        let border = Border {
            width: 1.,
            color: style.value,
            radius: (2.).into(),
        };

        let (cursor, offset, is_selecting) = if let Some(_focus) = state
            .is_focused
            .as_ref()
            .filter(|focus| focus.is_window_focused)
        {
            match state.cursor.state(value) {
                CursorState::Index(position) => {
                    let (text_value_width, offset) =
                        measure_cursor_and_scroll_offset(state.value.raw(), text_bounds, position);
                    let (text_value_width_n1, _offset_n1) =
                        measure_cursor_and_scroll_offset(state.value.raw(), text_bounds, position + 1);

                    let width = if self.mode == ControlMode::Insert {
                        if renderer::CRISP {
                            (1.0 / renderer.scale_factor().unwrap_or(1.0)).max(1.0)
                        } else {
                            1.0
                        }
                    } else {
                        if text_value_width_n1 == text_value_width {
                            10.0
                        } else {
                            text_value_width_n1 - text_value_width
                        }
                    };

                    let cursor = if !is_disabled {
                        Some((
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: text_bounds.x + text_value_width,
                                    y: text_bounds.y,
                                    width,
                                    height: text_bounds.height,
                                },
                                border,
                                ..renderer::Quad::default()
                            },
                            style.cursor,
                        ))
                    } else {
                        None
                    };

                    (cursor, offset, false)
                }
                CursorState::Selection { start, end } => {
                    let left = start.min(end);
                    let right = end.max(start);

                    let (left_position, left_offset) =
                        measure_cursor_and_scroll_offset(state.value.raw(), text_bounds, left);

                    let (right_position, right_offset) =
                        measure_cursor_and_scroll_offset(state.value.raw(), text_bounds, right);

                    let width = right_position - left_position;

                    (
                        Some((
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: text_bounds.x + left_position,
                                    y: text_bounds.y,
                                    width,
                                    height: text_bounds.height,
                                },
                                border,
                                ..renderer::Quad::default()
                            },
                            style.selection,
                        )),
                        if end == right {
                            right_offset
                        } else {
                            left_offset
                        },
                        true,
                    )
                }
            }
        } else {
            (None, 0.0, false)
        };

        let draw = |renderer: &mut Renderer, viewport| {
            let paragraph = if text.is_empty()
                && state
                    .preedit
                    .as_ref()
                    .map(|preedit| preedit.content.is_empty())
                    .unwrap_or(true)
            {
                state.placeholder.raw()
            } else {
                state.value.raw()
            };

            let alignment_offset =
                alignment_offset(text_bounds.width, paragraph.min_width(), self.alignment);

            for annotation in &self.annotations {
                let paragraph = state.value.raw();
                let (start_x, _) = measure_cursor_and_scroll_offset(paragraph, text_bounds, annotation.s);
                let (end_x, _) = measure_cursor_and_scroll_offset(paragraph, text_bounds, annotation.e);

                renderer.with_translation(Vector::new(alignment_offset - offset, 0.0), |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: text_bounds.x + start_x.max(0.),
                                y: text_bounds.y + text_bounds.height - 2.0,
                                width: (end_x - start_x).max(0.),
                                height: 2.0,
                            },
                            ..renderer::Quad::default()
                        },
                        Color::from_rgb(0.9, 0.4, 0.3),
                    );
                });
            }

            if let Some((cursor, color)) = cursor {
                renderer.with_translation(
                    Vector::new(alignment_offset - offset, 0.0),
                    |renderer| {
                        renderer.fill_quad(cursor, color);
                    },
                );
            } else {
                renderer.with_translation(Vector::ZERO, |_| {}); // ???
            }

            renderer.fill_paragraph(
                paragraph,
                text_bounds.anchor(paragraph.min_bounds(), Alignment::Start, Alignment::Center)
                    + Vector::new(alignment_offset - offset, 0.0),
                if text.is_empty() {
                    style.placeholder
                } else {
                    style.value
                },
                viewport,
            );
        };

        if is_selecting {
            renderer.with_layer(text_bounds, |renderer| draw(renderer, *viewport));
        } else {
            draw(renderer, text_bounds);
        }
    }

    fn handle_event(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let update_cache = |state, value| {
            replace_paragraph(
                renderer,
                state,
                layout,
                value,
                self.font,
                self.size,
                self.line_height,
            );
        };

        match &event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                let state = state::<Renderer>(tree);
                let cursor_before = state.cursor;

                let click_position = cursor.position_over(layout.bounds());

                state.is_focused = if click_position.is_some() {
                    let now = Instant::now();

                    Some(Focus {
                        updated_at: now,
                        now,
                        is_window_focused: true,
                    })
                } else {
                    None
                };

                if let Some(cursor_position) = click_position {
                    let text_layout = layout.children().next().unwrap();

                    let target = {
                        let text_bounds = text_layout.bounds();

                        let alignment_offset = alignment_offset(
                            text_bounds.width,
                            state.value.raw().min_width(),
                            self.alignment,
                        );

                        cursor_position.x - text_bounds.x - alignment_offset
                    };

                    let click =
                        mouse::Click::new(cursor_position, mouse::Button::Left, state.last_click);

                    match click.kind() {
                        click::Kind::Single => {
                            let position = if target > 0.0 {
                                let value = self.value.clone();

                                find_cursor_position(text_layout.bounds(), &value, state, target)
                            } else {
                                None
                            }
                            .unwrap_or(0);

                            if state.keyboard_modifiers.shift() {
                                state
                                    .cursor
                                    .select_range(state.cursor.start(&self.value), position);
                            } else {
                                state.cursor.move_to(position);
                            }

                            state.is_dragging = Some(Drag::Select);
                        }
                        click::Kind::Double => {
                            let position = find_cursor_position(
                                text_layout.bounds(),
                                &self.value,
                                state,
                                target,
                            )
                            .unwrap_or(0);

                            state.cursor.select_range(
                                self.value.previous_start_of_word(position),
                                self.value.next_end_of_word(position),
                            );

                            state.is_dragging = Some(Drag::SelectWords { anchor: position });
                        }
                        click::Kind::Triple => {
                            state.cursor.select_all(&self.value);
                            state.is_dragging = None;
                        }
                    }

                    state.last_click = Some(click);

                    if cursor_before != state.cursor {
                        shell.request_redraw();
                    }

                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                state::<Renderer>(tree).is_dragging = None;
            }
            Event::Mouse(mouse::Event::CursorMoved { position })
            | Event::Touch(touch::Event::FingerMoved { position, .. }) => {
                let state = state::<Renderer>(tree);

                if let Some(is_dragging) = &state.is_dragging {
                    let text_layout = layout.children().next().unwrap();

                    let target = {
                        let text_bounds = text_layout.bounds();

                        let alignment_offset = alignment_offset(
                            text_bounds.width,
                            state.value.raw().min_width(),
                            self.alignment,
                        );

                        position.x - text_bounds.x - alignment_offset
                    };

                    let value = self.value.clone();
                    let position =
                        find_cursor_position(text_layout.bounds(), &value, state, target)
                            .unwrap_or(0);

                    let selection_before = state.cursor.selection(&value);

                    match is_dragging {
                        Drag::Select => {
                            state
                                .cursor
                                .select_range(state.cursor.start(&value), position);
                        }
                        Drag::SelectWords { anchor } => {
                            if position < *anchor {
                                state.cursor.select_range(
                                    self.value.previous_start_of_word(position),
                                    self.value.next_end_of_word(*anchor),
                                );
                            } else {
                                state.cursor.select_range(
                                    self.value.previous_start_of_word(*anchor),
                                    self.value.next_end_of_word(position),
                                );
                            }
                        }
                    }

                    if let Some(focus) = &mut state.is_focused {
                        focus.updated_at = Instant::now();
                    }

                    if selection_before != state.cursor.selection(&value) {
                        shell.request_redraw();
                    }

                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modified_key,
                physical_key,
                ..
            }) => {
                let state = state::<Renderer>(tree);
                let modifiers = state.keyboard_modifiers;

                let Some(focus) = &mut state.is_focused 
                    else { return; };

                match self.mode {
                    ControlMode::Normal => {
                        match (state.keyboard_modifiers, modified_key.to_latin(*physical_key)) {
                            (Modifiers::NONE,  Some('h')) => state.cursor.move_left(&self.value),
                            (Modifiers::NONE,  Some('b')) => state.cursor.move_left_by_words(&self.value),
                            (Modifiers::NONE,  Some('l')) => state.cursor.move_right(&self.value),
                            (Modifiers::NONE,  Some('w')) => state.cursor.move_right_by_words(&self.value),
                            (Modifiers::SHIFT, Some('$')) => state.cursor.move_to(usize::MAX),
                            (Modifiers::NONE,  Some('0')) => state.cursor.move_to(0),
                            _ => (),
                        }
                    },
                    ControlMode::Insert => {
                        match key.to_latin(*physical_key) {
                            Some('c') if state.keyboard_modifiers.command() => {
                                if let Some((start, end)) = state.cursor.selection(&self.value) {
                                    shell.write_clipboard(clipboard::Content::Text(
                                        self.value.select(start, end).to_string(),
                                    ));
                                }

                                shell.capture_event();
                                return;
                            }
                            Some('x') if state.keyboard_modifiers.command() => {
                                let Some(on_input) = &self.on_input else {
                                    return;
                                };

                                if let Some((start, end)) = state.cursor.selection(&self.value) {
                                    shell.write_clipboard(clipboard::Content::Text(
                                        self.value.select(start, end).to_string(),
                                    ));
                                }

                                let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                                editor.delete();

                                let message = (on_input)(editor.contents());
                                shell.publish(message);
                                shell.capture_event();

                                focus.updated_at = Instant::now();
                                update_cache(state, &self.value);
                                return;
                            }
                            Some('v')
                                if state.keyboard_modifiers.command()
                                    && !state.keyboard_modifiers.alt() =>
                            {
                                let Some(on_input) = &self.on_input else {
                                    return;
                                };

                                let content = match &state.is_pasting {
                                    Some(Paste::Pasting(content)) => content,
                                    Some(Paste::Reading) => return,
                                    None => {
                                        shell.read_clipboard(clipboard::Kind::Text);
                                        state.is_pasting = Some(Paste::Reading);
                                        return;
                                    }
                                };

                                let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                                editor.paste(content.clone());

                                let message = if let Some(paste) = &self.on_paste {
                                    (paste)(editor.contents())
                                } else {
                                    (on_input)(editor.contents())
                                };
                                shell.publish(message);
                                shell.capture_event();

                                focus.updated_at = Instant::now();
                                update_cache(state, &self.value);
                                return;
                            }
                            Some('a') if state.keyboard_modifiers.command() => {
                                let cursor_before = state.cursor;

                                state.cursor.select_all(&self.value);

                                if cursor_before != state.cursor {
                                    focus.updated_at = Instant::now();

                                    shell.request_redraw();
                                }

                                shell.capture_event();
                                return;
                            }
                            _ => {}
                        }

                        if let Some(text) = text {
                            let Some(on_input) = &self.on_input else {
                                return;
                            };

                            state.is_pasting = None;

                            if let Some(c) = text.chars().next().filter(|c| !c.is_control()) {
                                let mut editor = Editor::new(&mut self.value, &mut state.cursor);

                                editor.insert(c);

                                let message = (on_input)(editor.contents());
                                shell.publish(message);
                                shell.capture_event();

                                focus.updated_at = Instant::now();
                                update_cache(state, &self.value);
                                return;
                            }
                        }

                        match modified_key.as_ref() {
                            keyboard::Key::Named(key::Named::Enter) => {
                                if let Some(on_submit) = self.on_submit.clone() {
                                    shell.publish(on_submit);
                                    shell.capture_event();
                                }
                            }
                            keyboard::Key::Named(key::Named::Backspace) => {
                                let Some(on_input) = &self.on_input else {
                                    return;
                                };

                                if state.cursor.selection(&self.value).is_none() {
                                    if (modifiers.jump()) || modifiers.macos_command()
                                    {
                                        state
                                            .cursor
                                            .select_range(state.cursor.start(&self.value), 0);
                                    } else if modifiers.jump() {
                                        state.cursor.select_left_by_words(&self.value);
                                    }
                                }

                                let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                                editor.backspace();

                                let message = (on_input)(editor.contents());
                                shell.publish(message);
                                shell.capture_event();

                                focus.updated_at = Instant::now();
                                update_cache(state, &self.value);
                            }
                            keyboard::Key::Named(key::Named::Delete) => {
                                let Some(on_input) = &self.on_input else {
                                    return;
                                };

                                if state.cursor.selection(&self.value).is_none() {
                                    if (modifiers.jump()) || modifiers.macos_command()
                                    {
                                        state.cursor.select_range(
                                            state.cursor.start(&self.value),
                                            self.value.len(),
                                        );
                                    } else if modifiers.jump() {
                                        state.cursor.select_right_by_words(&self.value);
                                    }
                                }

                                let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                                editor.delete();

                                let message = (on_input)(editor.contents());
                                shell.publish(message);
                                shell.capture_event();

                                focus.updated_at = Instant::now();
                                update_cache(state, &self.value);
                            }
                            keyboard::Key::Named(key::Named::Home) => {
                                let cursor_before = state.cursor;

                                if modifiers.shift() {
                                    state
                                        .cursor
                                        .select_range(state.cursor.start(&self.value), 0);
                                } else {
                                    state.cursor.move_to(0);
                                }

                                if cursor_before != state.cursor {
                                    focus.updated_at = Instant::now();

                                    shell.request_redraw();
                                }

                                shell.capture_event();
                            }
                            keyboard::Key::Named(key::Named::End) => {
                                let cursor_before = state.cursor;

                                if modifiers.shift() {
                                    state.cursor.select_range(
                                        state.cursor.start(&self.value),
                                        self.value.len(),
                                    );
                                } else {
                                    state.cursor.move_to(self.value.len());
                                }

                                if cursor_before != state.cursor {
                                    focus.updated_at = Instant::now();

                                    shell.request_redraw();
                                }

                                shell.capture_event();
                            }
                            keyboard::Key::Named(key::Named::ArrowLeft) => {
                                let cursor_before = state.cursor;

                                if (modifiers.jump()) || modifiers.macos_command() {
                                    if modifiers.shift() {
                                        state
                                            .cursor
                                            .select_range(state.cursor.start(&self.value), 0);
                                    } else {
                                        state.cursor.move_to(0);
                                    }
                                } else if modifiers.jump() {
                                    if modifiers.shift() {
                                        state.cursor.select_left_by_words(&self.value);
                                    } else {
                                        state.cursor.move_left_by_words(&self.value);
                                    }
                                } else if modifiers.shift() {
                                    state.cursor.select_left(&self.value);
                                } else {
                                    state.cursor.move_left(&self.value);
                                }

                                if cursor_before != state.cursor {
                                    focus.updated_at = Instant::now();

                                    shell.request_redraw();
                                }

                                shell.capture_event();
                            }
                            keyboard::Key::Named(key::Named::ArrowRight) => {
                                let cursor_before = state.cursor;

                                if (modifiers.jump()) || modifiers.macos_command() {
                                    if modifiers.shift() {
                                        state.cursor.select_range(
                                            state.cursor.start(&self.value),
                                            self.value.len(),
                                        );
                                    } else {
                                        state.cursor.move_to(self.value.len());
                                    }
                                } else if modifiers.jump() {
                                    if modifiers.shift() {
                                        state.cursor.select_right_by_words(&self.value);
                                    } else {
                                        state.cursor.move_right_by_words(&self.value);
                                    }
                                } else if modifiers.shift() {
                                    state.cursor.select_right(&self.value);
                                } else {
                                    state.cursor.move_right(&self.value);
                                }

                                if cursor_before != state.cursor {
                                    focus.updated_at = Instant::now();

                                    shell.request_redraw();
                                }

                                shell.capture_event();
                            }
                            keyboard::Key::Named(key::Named::Escape) => {
                                //state.is_focused = None;
                                state.is_dragging = None;
                                state.is_pasting = None;

                                state.keyboard_modifiers = keyboard::Modifiers::default();

                                shell.capture_event();
                            }
                            _ => {}
                        }
                    }
                    ControlMode::Term => (),
                }
            }
            Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                let state = state::<Renderer>(tree);

                if state.is_focused.is_some()
                    && let keyboard::Key::Character("v") = key.as_ref()
                {
                    state.is_pasting = None;

                    shell.capture_event();
                }

                state.is_pasting = None;
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                let state = state::<Renderer>(tree);

                state.keyboard_modifiers = *modifiers;
            }
            Event::InputMethod(event) => match event {
                input_method::Event::Opened | input_method::Event::Closed => {
                    let state = state::<Renderer>(tree);

                    state.preedit = matches!(event, input_method::Event::Opened)
                        .then(input_method::Preedit::new);

                    shell.request_redraw();
                }
                input_method::Event::Preedit(content, selection) => {
                    let state = state::<Renderer>(tree);

                    if state.is_focused.is_some() {
                        state.preedit = Some(input_method::Preedit {
                            content: content.to_owned(),
                            selection: selection.clone(),
                            text_size: self.size,
                        });

                        shell.request_redraw();
                    }
                }
                input_method::Event::Commit(text) => {
                    let state = state::<Renderer>(tree);

                    if let Some(focus) = &mut state.is_focused {
                        let Some(on_input) = &self.on_input else {
                            return;
                        };

                        let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                        editor.paste(Value::new(text));

                        focus.updated_at = Instant::now();
                        state.is_pasting = None;

                        let message = (on_input)(editor.contents());
                        shell.publish(message);
                        shell.capture_event();

                        update_cache(state, &self.value);
                    }
                }
            },
            Event::Window(window::Event::Unfocused) => {
                let state = state::<Renderer>(tree);

                if let Some(focus) = &mut state.is_focused {
                    focus.is_window_focused = false;
                }
            }
            Event::Window(window::Event::Focused) => {
                let state = state::<Renderer>(tree);

                if let Some(focus) = &mut state.is_focused {
                    focus.is_window_focused = true;
                    focus.updated_at = Instant::now();

                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(_now)) => (),
            _ => {}
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TextInput<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::new(self.mode))
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();

        // Stop pasting if input becomes disabled
        if self.on_input.is_none() {
            state.is_pasting = None;
        }
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.layout(tree, renderer, limits, None)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();

        operation.text_input(self.id.as_ref(), layout.bounds(), state);
        operation.focusable(self.id.as_ref(), layout.bounds(), state);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let (was_focused, was_dragging, was_pasting, prev_cursor) = {
            let state = state::<Renderer>(tree);
            (state.is_focused, state.is_dragging, matches!(state.is_pasting, Some(Paste::Pasting(_))), state.cursor)
        };

        self.handle_event(tree, event, layout, cursor, renderer, shell);

        let state = state::<Renderer>(tree);
        let is_disabled = self.on_input.is_none();

        let status = if is_disabled {
            Status::Disabled
        } else if state.is_focused() {
            Status::Focused {
                is_hovered: self.mode == ControlMode::Insert, //cursor.is_over(layout.bounds()),
            }
        } else if cursor.is_over(layout.bounds()) {
            Status::Hovered
        } else {
            Status::Active
        };

        let state_changed =
            state.is_focused != was_focused
            || state.is_dragging != was_dragging
            || matches!(state.is_pasting, Some(Paste::Pasting(_))) != was_pasting
            || state.cursor != prev_cursor;

        if let Event::Window(window::Event::RedrawRequested(_now)) = event {
            self.last_status = Some(status);
        } else if state_changed || self.last_status.is_some_and(|last_status| status != last_status) {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.draw(tree, renderer, theme, layout, cursor, None, viewport);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            if self.on_input.is_none() {
                mouse::Interaction::Idle
            } else {
                mouse::Interaction::Text
            }
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<TextInput<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: Catalog + 'a,
    Renderer: text::Renderer + 'a,
{
    fn from(
        text_input: TextInput<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(text_input)
    }
}

/// The content of the [`Icon`].
#[derive(Debug, Clone)]
pub struct Icon<Font> {
    /// The font that will be used to display the `code_point`.
    pub font: Font,
    /// The unicode code point that will be used as the icon.
    pub code_point: char,
    /// The font size of the content.
    pub size: Option<Pixels>,
    /// The spacing between the [`Icon`] and the text in a [`TextInput`].
    pub spacing: f32,
    /// The side of a [`TextInput`] where to display the [`Icon`].
    pub side: Side,
}

/// The side of a [`TextInput`].
#[derive(Debug, Clone)]
pub enum Side {
    /// The left side of a [`TextInput`].
    Left,
    /// The right side of a [`TextInput`].
    Right,
}

/// The state of a [`TextInput`].
#[derive(Debug, Clone)]
pub struct State<P: text::Paragraph> {
    value: paragraph::Plain<P>,
    placeholder: paragraph::Plain<P>,
    icon: paragraph::Plain<P>,
    is_focused: Option<Focus>,
    is_dragging: Option<Drag>,
    is_pasting: Option<Paste>,
    preedit: Option<input_method::Preedit>,
    last_click: Option<mouse::Click>,
    cursor: Cursor,
    keyboard_modifiers: keyboard::Modifiers,
    // TODO: Add stateful horizontal scrolling offset
}

fn state<Renderer: text::Renderer>(tree: &mut Tree) -> &mut State<Renderer::Paragraph> {
    tree.state.downcast_mut::<State<Renderer::Paragraph>>()
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Focus {
    updated_at: Instant,
    now: Instant,
    is_window_focused: bool,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Drag {
    Select,
    SelectWords { anchor: usize },
}

#[derive(Debug, Clone)]
enum Paste {
    Reading,
    Pasting(Value),
}

impl<P: text::Paragraph> State<P> {
    pub fn new(cstate: ControlMode) -> Self {
        let is_focused = match cstate {
            ControlMode::Term => None,
            ControlMode::Normal | ControlMode::Insert => {
                let now = Instant::now();
                Some(Focus {
                    updated_at: now,
                    now,
                    is_window_focused: true,
                })
            },
        };
        Self {
            value: Default::default(),
            placeholder: Default::default(),
            icon: Default::default(),
            is_focused,
            is_dragging: Default::default(),
            is_pasting: Default::default(),
            preedit: Default::default(),
            last_click: Default::default(),
            cursor: Default::default(),
            keyboard_modifiers: Default::default(),
        }
    }

    /// Returns whether the [`TextInput`] is currently focused or not.
    pub fn is_focused(&self) -> bool {
        self.is_focused.is_some()
    }

    /// Returns the [`Cursor`] of the [`TextInput`].
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Focuses the [`TextInput`].
    pub fn focus(&mut self) {
        let now = Instant::now();

        self.is_focused = Some(Focus {
            updated_at: now,
            now,
            is_window_focused: true,
        });

        self.move_cursor_to_end();
    }

    /// Unfocuses the [`TextInput`].
    pub fn unfocus(&mut self) {
        self.is_focused = None;
    }

    /// Moves the [`Cursor`] of the [`TextInput`] to the front of the input text.
    pub fn move_cursor_to_front(&mut self) {
        self.cursor.move_to(0);
    }

    /// Moves the [`Cursor`] of the [`TextInput`] to the end of the input text.
    pub fn move_cursor_to_end(&mut self) {
        self.cursor.move_to(usize::MAX);
    }

    /// Moves the [`Cursor`] of the [`TextInput`] to an arbitrary location.
    pub fn move_cursor_to(&mut self, position: usize) {
        self.cursor.move_to(position);
    }

    /// Selects all the content of the [`TextInput`].
    pub fn select_all(&mut self) {
        self.cursor.select_range(0, usize::MAX);
    }

    /// Selects the given range of the content of the [`TextInput`].
    pub fn select_range(&mut self, start: usize, end: usize) {
        self.cursor.select_range(start, end);
    }
}

impl<P: text::Paragraph> operation::Focusable for State<P> {
    fn is_focused(&self) -> bool {
        State::is_focused(self)
    }

    fn focus(&mut self) {
        State::focus(self);
    }

    fn unfocus(&mut self) {
        State::unfocus(self);
    }
}

impl<P: text::Paragraph> operation::TextInput for State<P> {
    fn text(&self) -> &str {
        if self.value.content().is_empty() {
            self.placeholder.content()
        } else {
            self.value.content()
        }
    }

    fn move_cursor_to_front(&mut self) {
        State::move_cursor_to_front(self);
    }

    fn move_cursor_to_end(&mut self) {
        State::move_cursor_to_end(self);
    }

    fn move_cursor_to(&mut self, position: usize) {
        State::move_cursor_to(self, position);
    }

    fn select_all(&mut self) {
        State::select_all(self);
    }

    fn select_range(&mut self, start: usize, end: usize) {
        State::select_range(self, start, end);
    }
}

fn offset<P: text::Paragraph>(text_bounds: Rectangle, value: &Value, state: &State<P>) -> f32 {
    if state.is_focused() {
        let cursor = state.cursor();

        let focus_position = match cursor.state(value) {
            CursorState::Index(i) => i,
            CursorState::Selection { end, .. } => end,
        };

        let (_, offset) =
            measure_cursor_and_scroll_offset(state.value.raw(), text_bounds, focus_position);

        offset
    } else {
        0.0
    }
}

fn measure_cursor_and_scroll_offset(
    paragraph: &impl text::Paragraph,
    text_bounds: Rectangle,
    cursor_index: usize,
) -> (f32, f32) {
    let grapheme_position = paragraph
        .grapheme_position(0, cursor_index)
        .unwrap_or(Point::ORIGIN);

    let offset = ((grapheme_position.x + 5.0) - text_bounds.width).max(0.0);

    (grapheme_position.x, offset)
}

/// Computes the position of the text cursor at the given X coordinate of
/// a [`TextInput`].
fn find_cursor_position<P: text::Paragraph>(
    text_bounds: Rectangle,
    value: &Value,
    state: &State<P>,
    x: f32,
) -> Option<usize> {
    let offset = offset(text_bounds, value, state);
    let value = value.to_string();

    let char_offset = state
        .value
        .raw()
        .hit_test(Point::new(x + offset, text_bounds.height / 2.0))
        .map(text::Hit::cursor)?;

    Some(
        unicode_segmentation::UnicodeSegmentation::graphemes(
            &value[..char_offset.min(value.len())],
            true,
        )
        .count(),
    )
}

fn replace_paragraph<Renderer>(
    renderer: &Renderer,
    state: &mut State<Renderer::Paragraph>,
    layout: Layout<'_>,
    value: &Value,
    font: Option<Renderer::Font>,
    text_size: Option<Pixels>,
    line_height: text::LineHeight,
) where
    Renderer: text::Renderer,
{
    let font = font.unwrap_or_else(|| renderer.default_font());
    let text_size = text_size.unwrap_or_else(|| renderer.default_size());

    let mut children_layout = layout.children();
    let text_bounds = children_layout.next().unwrap().bounds();

    state.value = paragraph::Plain::new(Text {
        font,
        line_height,
        content: value.to_string(),
        bounds: Size::new(f32::INFINITY, text_bounds.height),
        size: text_size,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Center,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::default(),
        hint_factor: renderer.scale_factor(),
        ellipsis: Default::default(),
    });
}

/// The possible status of a [`TextInput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The [`TextInput`] can be interacted with.
    Active,
    /// The [`TextInput`] is being hovered.
    Hovered,
    /// The [`TextInput`] is focused.
    Focused {
        /// Whether the [`TextInput`] is hovered, while focused.
        is_hovered: bool,
    },
    /// The [`TextInput`] cannot be interacted with.
    Disabled,
}

/// The appearance of a text input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The [`Background`] of the text input.
    pub background: Background,
    /// The [`Border`] of the text input.
    pub border: Border,
    /// The [`Color`] of the icon of the text input.
    pub icon: Color,
    /// The [`Color`] of the placeholder of the text input.
    pub placeholder: Color,
    /// The [`Color`] of the value of the text input.
    pub value: Color,
    /// The [`Color`] of the value of the text input.
    pub cursor: Color,
    /// The [`Color`] of the selection of the text input.
    pub selection: Color,
}

/// The theme catalog of a [`TextInput`].
pub trait Catalog: Sized {
    /// The item class of the [`Catalog`].
    type Class<'a>;

    /// The default class produced by the [`Catalog`].
    fn default<'a>() -> Self::Class<'a>;

    /// The [`Style`] of a class with the given status.
    fn style(&self, class: &Self::Class<'_>, status: Status) -> Style;
}

/// A styling function for a [`TextInput`].
///
/// This is just a boxed closure: `Fn(&Theme, Status) -> Style`.
pub type StyleFn<'a, Theme> = Box<dyn Fn(&Theme, Status) -> Style + 'a>;

fn alignment_offset(
    text_bounds_width: f32,
    text_min_width: f32,
    alignment: alignment::Horizontal,
) -> f32 {
    if text_min_width > text_bounds_width {
        0.0
    } else {
        match alignment {
            alignment::Horizontal::Left => 0.0,
            alignment::Horizontal::Center => (text_bounds_width - text_min_width) / 2.0,
            alignment::Horizontal::Right => text_bounds_width - text_min_width,
        }
    }
}

/// The cursor of a text input.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Cursor {
    state: CursorState,
}

/// The state of a [`Cursor`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CursorState {
    /// Cursor without a selection
    Index(usize),

    /// Cursor selecting a range of text
    Selection {
        /// The start of the selection
        start: usize,
        /// The end of the selection
        end: usize,
    },
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            state: CursorState::Index(0),
        }
    }
}

impl Cursor {
    /// Returns the [`State`] of the [`Cursor`].
    pub fn state(&self, value: &Value) -> CursorState {
        match self.state {
            CursorState::Index(index) => CursorState::Index(index.min(value.len())),
            CursorState::Selection { start, end } => {
                let start = start.min(value.len());
                let end = end.min(value.len());

                if start == end {
                    CursorState::Index(start)
                } else {
                    CursorState::Selection { start, end }
                }
            }
        }
    }

    /// Returns the current selection of the [`Cursor`] for the given [`Value`].
    ///
    /// `start` is guaranteed to be <= than `end`.
    pub fn selection(&self, value: &Value) -> Option<(usize, usize)> {
        match self.state(value) {
            CursorState::Selection { start, end } => Some((start.min(end), start.max(end))),
            CursorState::Index(_) => None,
        }
    }

    pub(crate) fn move_to(&mut self, position: usize) {
        self.state = CursorState::Index(position);
    }

    pub(crate) fn move_right(&mut self, value: &Value) {
        self.move_right_by_amount(value, 1);
    }

    pub(crate) fn move_right_by_words(&mut self, value: &Value) {
        self.move_to(value.next_end_of_word(self.right(value)));
    }

    pub(crate) fn move_right_by_amount(&mut self, value: &Value, amount: usize) {
        match self.state(value) {
            CursorState::Index(index) => {
                self.move_to(index.saturating_add(amount).min(value.len()));
            }
            CursorState::Selection { start, end } => self.move_to(end.max(start)),
        }
    }

    pub(crate) fn move_left(&mut self, value: &Value) {
        match self.state(value) {
            CursorState::Index(index) if index > 0 => self.move_to(index - 1),
            CursorState::Selection { start, end } => self.move_to(start.min(end)),
            CursorState::Index(_) => self.move_to(0),
        }
    }

    pub(crate) fn move_left_by_words(&mut self, value: &Value) {
        self.move_to(value.previous_start_of_word(self.left(value)));
    }

    pub(crate) fn select_range(&mut self, start: usize, end: usize) {
        if start == end {
            self.state = CursorState::Index(start);
        } else {
            self.state = CursorState::Selection { start, end };
        }
    }

    pub(crate) fn select_left(&mut self, value: &Value) {
        match self.state(value) {
            CursorState::Index(index) if index > 0 => {
                self.select_range(index, index - 1);
            }
            CursorState::Selection { start, end } if end > 0 => {
                self.select_range(start, end - 1);
            }
            _ => {}
        }
    }

    pub(crate) fn select_right(&mut self, value: &Value) {
        match self.state(value) {
            CursorState::Index(index) if index < value.len() => {
                self.select_range(index, index + 1);
            }
            CursorState::Selection { start, end } if end < value.len() => {
                self.select_range(start, end + 1);
            }
            _ => {}
        }
    }

    pub(crate) fn select_left_by_words(&mut self, value: &Value) {
        match self.state(value) {
            CursorState::Index(index) => {
                self.select_range(index, value.previous_start_of_word(index));
            }
            CursorState::Selection { start, end } => {
                self.select_range(start, value.previous_start_of_word(end));
            }
        }
    }

    pub(crate) fn select_right_by_words(&mut self, value: &Value) {
        match self.state(value) {
            CursorState::Index(index) => {
                self.select_range(index, value.next_end_of_word(index));
            }
            CursorState::Selection { start, end } => {
                self.select_range(start, value.next_end_of_word(end));
            }
        }
    }

    pub(crate) fn select_all(&mut self, value: &Value) {
        self.select_range(0, value.len());
    }

    pub(crate) fn start(&self, value: &Value) -> usize {
        let start = match self.state {
            CursorState::Index(index) => index,
            CursorState::Selection { start, .. } => start,
        };

        start.min(value.len())
    }

    pub(crate) fn end(&self, value: &Value) -> usize {
        let end = match self.state {
            CursorState::Index(index) => index,
            CursorState::Selection { end, .. } => end,
        };

        end.min(value.len())
    }

    fn left(&self, value: &Value) -> usize {
        match self.state(value) {
            CursorState::Index(index) => index,
            CursorState::Selection { start, end } => start.min(end),
        }
    }

    fn right(&self, value: &Value) -> usize {
        match self.state(value) {
            CursorState::Index(index) => index,
            CursorState::Selection { start, end } => start.max(end),
        }
    }
}

pub struct Editor<'a> {
    value: &'a mut Value,
    cursor: &'a mut Cursor,
}

impl<'a> Editor<'a> {
    pub fn new(value: &'a mut Value, cursor: &'a mut Cursor) -> Editor<'a> {
        Editor { value, cursor }
    }

    pub fn contents(&self) -> String {
        self.value.to_string()
    }

    pub fn insert(&mut self, character: char) {
        if let Some((left, right)) = self.cursor.selection(self.value) {
            self.cursor.move_left(self.value);
            self.value.remove_many(left, right);
        }

        self.value.insert(self.cursor.end(self.value), character);
        self.cursor.move_right(self.value);
    }

    pub fn paste(&mut self, content: Value) {
        let length = content.len();
        if let Some((left, right)) = self.cursor.selection(self.value) {
            self.cursor.move_left(self.value);
            self.value.remove_many(left, right);
        }

        self.value.insert_many(self.cursor.end(self.value), content);

        self.cursor.move_right_by_amount(self.value, length);
    }

    pub fn backspace(&mut self) {
        match self.cursor.selection(self.value) {
            Some((start, end)) => {
                self.cursor.move_left(self.value);
                self.value.remove_many(start, end);
            }
            None => {
                let start = self.cursor.start(self.value);

                if start > 0 {
                    self.cursor.move_left(self.value);
                    self.value.remove(start - 1);
                }
            }
        }
    }

    pub fn delete(&mut self) {
        match self.cursor.selection(self.value) {
            Some(_) => {
                self.backspace();
            }
            None => {
                let end = self.cursor.end(self.value);

                if end < self.value.len() {
                    self.value.remove(end);
                }
            }
        }
    }
}

/// The value of a [`TextInput`].
///
/// [`TextInput`]: super::TextInput
// TODO: Reduce allocations, cache results (?)
#[derive(Debug, Clone)]
pub struct Value {
    graphemes: Vec<String>,
}

impl Value {
    /// Creates a new [`Value`] from a string slice.
    pub fn new(string: &str) -> Self {
        let graphemes = UnicodeSegmentation::graphemes(string, true)
            .map(String::from)
            .collect();

        Self { graphemes }
    }

    /// Returns whether the [`Value`] is empty or not.
    ///
    /// A [`Value`] is empty when it contains no graphemes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the total amount of graphemes in the [`Value`].
    pub fn len(&self) -> usize {
        self.graphemes.len()
    }

    /// Returns the position of the previous start of a word from the given
    /// grapheme `index`.
    pub fn previous_start_of_word(&self, index: usize) -> usize {
        let previous_string = &self.graphemes[..index.min(self.graphemes.len())].concat();

        UnicodeSegmentation::split_word_bound_indices(previous_string as &str)
            .rfind(|(_, word)| !word.trim_start().is_empty())
            .map(|(i, previous_word)| {
                index
                    - UnicodeSegmentation::graphemes(previous_word, true).count()
                    - UnicodeSegmentation::graphemes(
                        &previous_string[i + previous_word.len()..] as &str,
                        true,
                    )
                    .count()
            })
            .unwrap_or(0)
    }

    /// Returns the position of the next end of a word from the given grapheme
    /// `index`.
    pub fn next_end_of_word(&self, index: usize) -> usize {
        let next_string = &self.graphemes[index..].concat();

        UnicodeSegmentation::split_word_bound_indices(next_string as &str)
            .find(|(_, word)| !word.trim_start().is_empty())
            .map(|(i, next_word)| {
                index
                    + UnicodeSegmentation::graphemes(next_word, true).count()
                    + UnicodeSegmentation::graphemes(&next_string[..i] as &str, true).count()
            })
            .unwrap_or(self.len())
    }

    /// Returns a new [`Value`] containing the graphemes from `start` until the
    /// given `end`.
    pub fn select(&self, start: usize, end: usize) -> Self {
        let graphemes = self.graphemes[start.min(self.len())..end.min(self.len())].to_vec();

        Self { graphemes }
    }

    /// Returns a new [`Value`] containing the graphemes until the given
    /// `index`.
    pub fn until(&self, index: usize) -> Self {
        let graphemes = self.graphemes[..index.min(self.len())].to_vec();

        Self { graphemes }
    }

    /// Inserts a new `char` at the given grapheme `index`.
    pub fn insert(&mut self, index: usize, c: char) {
        self.graphemes.insert(index, c.to_string());

        self.graphemes = UnicodeSegmentation::graphemes(&self.to_string() as &str, true)
            .map(String::from)
            .collect();
    }

    /// Inserts a bunch of graphemes at the given grapheme `index`.
    pub fn insert_many(&mut self, index: usize, mut value: Value) {
        let _ = self
            .graphemes
            .splice(index..index, value.graphemes.drain(..));
    }

    /// Removes the grapheme at the given `index`.
    pub fn remove(&mut self, index: usize) {
        let _ = self.graphemes.remove(index);
    }

    /// Removes the graphemes from `start` to `end`.
    pub fn remove_many(&mut self, start: usize, end: usize) {
        let _ = self.graphemes.splice(start..end, std::iter::empty());
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.graphemes.concat())
    }
}
