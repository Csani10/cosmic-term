// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    iced::{
        advanced::graphics::text::{font_system, Raw},
        event::{Event, Status},
        keyboard::{Event as KeyEvent, KeyCode, Modifiers},
        mouse::{self, Button, Event as MouseEvent, ScrollDelta},
        Color, Element, Length, Padding, Point, Rectangle, Size, Vector,
    },
    iced_core::{
        clipboard::Clipboard,
        image,
        layout::{self, Layout},
        renderer::{self, Quad},
        text,
        widget::{self, tree, Widget},
        Shell,
    },
};
use cosmic_text::{Action, Edit, Metrics, Motion, Scroll};
use std::{
    cell::Cell,
    cmp,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{Terminal, TerminalScroll};

pub struct TerminalBox<'a, Message> {
    terminal: &'a Mutex<Terminal>,
    padding: Padding,
    click_timing: Duration,
    context_menu: Option<Point>,
    on_context_menu: Option<Box<dyn Fn(Option<Point>) -> Message + 'a>>,
}

impl<'a, Message> TerminalBox<'a, Message>
where
    Message: Clone,
{
    pub fn new(terminal: &'a Mutex<Terminal>) -> Self {
        Self {
            terminal,
            padding: Padding::new(0.0),
            click_timing: Duration::from_millis(500),
            context_menu: None,
            on_context_menu: None,
        }
    }

    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn click_timing(mut self, click_timing: Duration) -> Self {
        self.click_timing = click_timing;
        self
    }

    pub fn context_menu(mut self, position: Point) -> Self {
        self.context_menu = Some(position);
        self
    }

    pub fn on_context_menu(
        mut self,
        on_context_menu: impl Fn(Option<Point>) -> Message + 'a,
    ) -> Self {
        self.on_context_menu = Some(Box::new(on_context_menu));
        self
    }
}

pub fn terminal_box<'a, Message>(terminal: &'a Mutex<Terminal>) -> TerminalBox<'a, Message>
where
    Message: Clone,
{
    TerminalBox::new(terminal)
}

impl<'a, Message, Renderer> Widget<Message, Renderer> for TerminalBox<'a, Message>
where
    Message: Clone,
    Renderer:
        renderer::Renderer + image::Renderer<Handle = image::Handle> + text::Renderer<Raw = Raw>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn width(&self) -> Length {
        Length::Fill
    }

    fn height(&self) -> Length {
        Length::Fill
    }

    fn layout(
        &self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);

        let mut terminal = self.terminal.lock().unwrap();
        //TODO: set size?
        terminal.with_buffer_mut(|buffer| {
            let mut font_system = font_system().write().unwrap();
            buffer.shape_until_scroll(font_system.raw(), true);
        });

        terminal.with_buffer(|buffer| {
            let mut layout_lines = 0;
            for line in buffer.lines.iter() {
                match line.layout_opt() {
                    Some(layout) => layout_lines += layout.len(),
                    None => (),
                }
            }

            let height = layout_lines as f32 * buffer.metrics().line_height;
            let size = Size::new(limits.max().width, height);

            layout::Node::new(limits.resolve(size))
        })
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();

        match &state.dragging {
            Some(Dragging::Scrollbar { .. }) => return mouse::Interaction::Idle,
            _ => {}
        }

        if let Some(p) = cursor_position.position_in(layout.bounds()) {
            let scale_factor = state.scale_factor.get();
            let terminal = self.terminal.lock().unwrap();
            let buffer_size = terminal.with_buffer(|buffer| buffer.size());

            let x_logical = p.x - self.padding.left;
            let y_logical = p.y - self.padding.top;
            let x = x_logical * scale_factor;
            let y = y_logical * scale_factor;
            if x >= 0.0 && x < buffer_size.0 && y >= 0.0 && y < buffer_size.1 {
                return mouse::Interaction::Text;
            }
        }

        mouse::Interaction::Idle
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Renderer::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor_position: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let instant = Instant::now();

        let state = tree.state.downcast_ref::<State>();

        let mut terminal = self.terminal.lock().unwrap();

        //TODO: make this configurable
        let scrollbar_w = 8.0;

        let view_position =
            layout.position() + [self.padding.left as f32, self.padding.top as f32].into();
        let view_w = cmp::min(viewport.width as i32, layout.bounds().width as i32)
            - self.padding.horizontal() as i32
            - scrollbar_w as i32;
        let view_h = cmp::min(viewport.height as i32, layout.bounds().height as i32)
            - self.padding.vertical() as i32;
        let scale_factor = style.scale_factor as f32;

        if view_w <= 0 || view_h <= 0 {
            // Zero sized image
            return;
        }

        // Ensure terminal is the right size
        terminal.resize(view_w as u32, view_h as u32, scale_factor);

        // Cache scale factor
        state.scale_factor.set(scale_factor);

        // Ensure terminal is shaped
        terminal.with_buffer_mut(|buffer| {
            let mut font_system = font_system().write().unwrap();
            buffer.shape_until_scroll(font_system.raw(), true);
        });

        // Render default background
        {
            let background_color = cosmic_text::Color(terminal.default_attrs().metadata as u32);
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(view_position, Size::new(view_w as f32, view_h as f32)),
                    border_radius: 0.0.into(),
                    border_width: 0.0,
                    border_color: Color::TRANSPARENT,
                },
                Color::new(
                    background_color.r() as f32 / 255.0,
                    background_color.g() as f32 / 255.0,
                    background_color.b() as f32 / 255.0,
                    background_color.a() as f32 / 255.0,
                ),
            );
        }

        // Render cell backgrounds that do not match default
        terminal.with_buffer(|buffer| {
            let line_height = buffer.metrics().line_height;
            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    if glyph.metadata != terminal.default_attrs().metadata {
                        let background_color = cosmic_text::Color(glyph.metadata as u32);
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle::new(
                                    view_position + Vector::new(glyph.x, run.line_top),
                                    Size::new(glyph.w, line_height),
                                ),
                                border_radius: 0.0.into(),
                                border_width: 0.0,
                                border_color: Color::TRANSPARENT,
                            },
                            Color::new(
                                background_color.r() as f32 / 255.0,
                                background_color.g() as f32 / 255.0,
                                background_color.b() as f32 / 255.0,
                                background_color.a() as f32 / 255.0,
                            ),
                        );
                    }
                }
            }
        });

        renderer.fill_raw(Raw {
            buffer: terminal.buffer_weak(),
            position: view_position,
            color: Color::new(1.0, 1.0, 1.0, 1.0), // TODO
            clip_bounds: Rectangle::new(view_position, Size::new(view_w as f32, view_h as f32)),
        });

        // Draw scrollbar
        let (start, end) = terminal.scrollbar();
        let scrollbar_y = start * view_h as f32;
        let scrollbar_h = end * view_h as f32 - scrollbar_y;
        let scrollbar_rect = Rectangle::new(
            [view_w as f32, scrollbar_y].into(),
            Size::new(scrollbar_w, scrollbar_h),
        );
        let scrollbar_alpha = match &state.dragging {
            Some(Dragging::Scrollbar { .. }) => 0.5,
            _ => 0.25,
        };
        renderer.fill_quad(
            Quad {
                bounds: scrollbar_rect + Vector::new(view_position.x, view_position.y),
                border_radius: 0.0.into(),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
            Color::new(1.0, 1.0, 1.0, scrollbar_alpha),
        );
        state.scrollbar_rect.set(scrollbar_rect);

        let duration = instant.elapsed();
        log::debug!("redraw {}, {}: {:?}", view_w, view_h, duration);
    }

    fn on_event(
        &mut self,
        tree: &mut widget::Tree,
        event: Event,
        layout: Layout<'_>,
        cursor_position: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle<f32>,
    ) -> Status {
        let state = tree.state.downcast_mut::<State>();
        let scale_factor = state.scale_factor.get();
        let scrollbar_rect = state.scrollbar_rect.get();
        let mut terminal = self.terminal.lock().unwrap();
        let buffer_size = terminal.with_buffer(|buffer| buffer.size());
        let mut font_system = font_system().write().unwrap();

        let mut status = Status::Ignored;
        match event {
            //TODO: Alt keys when they are control characters
            Event::Keyboard(KeyEvent::KeyPressed {
                key_code,
                modifiers,
            }) => match key_code {
                KeyCode::Backspace => {
                    terminal.input(b"\x08".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Tab => {
                    if modifiers.shift() {
                        terminal.input(b"\x1B[Z".as_slice());
                    } else {
                        terminal.input(b"\t".as_slice());
                    }
                    status = Status::Captured;
                }
                KeyCode::Enter => {
                    terminal.input(b"\n".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Escape => {
                    terminal.input(b"\x1B".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Up => {
                    terminal.input(b"\x1B[A".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Down => {
                    terminal.input(b"\x1B[B".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Right => {
                    terminal.input(b"\x1B[C".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Left => {
                    terminal.input(b"\x1B[D".as_slice());
                    status = Status::Captured;
                }
                KeyCode::End => {
                    if modifiers.shift() {
                        terminal.scroll(TerminalScroll::Bottom);
                    } else {
                        terminal.input(b"\x1B[F".as_slice());
                    }
                    status = Status::Captured;
                }
                KeyCode::Home => {
                    if modifiers.shift() {
                        terminal.scroll(TerminalScroll::Top);
                    } else {
                        terminal.input(b"\x1B[H".as_slice());
                    }
                    status = Status::Captured;
                }
                KeyCode::Insert => {
                    terminal.input(b"\x1B[2~".as_slice());
                    status = Status::Captured;
                }
                KeyCode::Delete => {
                    terminal.input(b"\x1B[3~".as_slice());
                    status = Status::Captured;
                }
                KeyCode::PageUp => {
                    if modifiers.shift() {
                        terminal.scroll(TerminalScroll::PageUp);
                    } else {
                        terminal.input(b"\x1B[5~".as_slice());
                    }
                    status = Status::Captured;
                }
                KeyCode::PageDown => {
                    if modifiers.shift() {
                        terminal.scroll(TerminalScroll::PageDown);
                    } else {
                        terminal.input(b"\x1B[6~".as_slice());
                    }
                    status = Status::Captured;
                }
                //TODO: F1-F12 keys
                _ => (),
            },
            Event::Keyboard(KeyEvent::ModifiersChanged(modifiers)) => {
                state.modifiers = modifiers;
            }
            Event::Keyboard(KeyEvent::CharacterReceived(character)) => {
                match (
                    state.modifiers.logo(),
                    state.modifiers.control(),
                    state.modifiers.alt(),
                ) {
                    (true, _, _) => {
                        // Ignore super
                    }
                    (false, true, _) => {
                        // Handle ctrl for control characters (Ctrl-A to Ctrl-Z)
                        if character.is_control() {
                            let mut buf = [0, 0, 0, 0];
                            let str = character.encode_utf8(&mut buf);
                            terminal.input(str.as_bytes().to_vec());
                        }
                    }
                    (false, false, true) => {
                        if !character.is_control() {
                            // Handle alt for non-control characters
                            let mut buf = [0x1B, 0, 0, 0, 0];
                            let str = character.encode_utf8(&mut buf[1..]);
                            terminal.input(str.as_bytes().to_vec());
                        }
                    }
                    (false, false, false) => {
                        // Handle no modifiers for non-control characters
                        if !character.is_control() {
                            let mut buf = [0, 0, 0, 0];
                            let str = character.encode_utf8(&mut buf);
                            terminal.input(str.as_bytes().to_vec());
                        }
                    }
                }
                status = Status::Captured;
            }
            Event::Mouse(MouseEvent::ButtonPressed(button)) => {
                if let Some(p) = cursor_position.position_in(layout.bounds()) {
                    // Handle left click drag
                    if let Button::Left = button {
                        let x_logical = p.x - self.padding.left;
                        let y_logical = p.y - self.padding.top;
                        let x = x_logical * scale_factor;
                        let y = y_logical * scale_factor;
                        if x >= 0.0 && x < buffer_size.0 && y >= 0.0 && y < buffer_size.1 {
                            let click_kind =
                                if let Some((click_kind, click_time)) = state.click.take() {
                                    if click_time.elapsed() < self.click_timing {
                                        match click_kind {
                                            ClickKind::Single => ClickKind::Double,
                                            ClickKind::Double => ClickKind::Triple,
                                            ClickKind::Triple => ClickKind::Single,
                                        }
                                    } else {
                                        ClickKind::Single
                                    }
                                } else {
                                    ClickKind::Single
                                };
                            /*TODO
                            match click_kind {
                                ClickKind::Single => editor.action(Action::Click {
                                    x: x as i32,
                                    y: y as i32,
                                }),
                                ClickKind::Double => editor.action(Action::DoubleClick {
                                    x: x as i32,
                                    y: y as i32,
                                }),
                                ClickKind::Triple => editor.action(Action::TripleClick {
                                    x: x as i32,
                                    y: y as i32,
                                }),
                            }
                            */
                            state.click = Some((click_kind, Instant::now()));
                            state.dragging = Some(Dragging::Buffer);
                        } else if scrollbar_rect.contains(Point::new(x_logical, y_logical)) {
                            state.dragging = Some(Dragging::Scrollbar {
                                start_y: y,
                                start_scroll: terminal.scrollbar(),
                            });
                        } else if x_logical >= scrollbar_rect.x
                            && x_logical < (scrollbar_rect.x + scrollbar_rect.width)
                        {
                            let scroll_ratio = terminal.with_buffer(|buffer| y / buffer.size().1);
                            terminal.scroll_to(scroll_ratio);
                            state.dragging = Some(Dragging::Scrollbar {
                                start_y: y,
                                start_scroll: terminal.scrollbar(),
                            });
                        }
                    }

                    // Update context menu state
                    if let Some(on_context_menu) = &self.on_context_menu {
                        shell.publish((on_context_menu)(match self.context_menu {
                            Some(_) => None,
                            None => match button {
                                Button::Right => Some(p),
                                _ => None,
                            },
                        }));
                    }

                    status = Status::Captured;
                }
            }
            Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                state.dragging = None;
                status = Status::Captured;
            }
            Event::Mouse(MouseEvent::CursorMoved { .. }) => {
                if let Some(dragging) = &state.dragging {
                    if let Some(p) = cursor_position.position() {
                        let x_logical = (p.x - layout.bounds().x) - self.padding.left;
                        let y_logical = (p.y - layout.bounds().y) - self.padding.top;
                        let x = x_logical * scale_factor;
                        let y = y_logical * scale_factor;
                        match dragging {
                            Dragging::Buffer => {
                                /*TODO
                                editor.action(Action::Drag {
                                    x: x as i32,
                                    y: y as i32,
                                });
                                */
                            }
                            Dragging::Scrollbar {
                                start_y,
                                start_scroll,
                            } => {
                                let scroll_offset = terminal
                                    .with_buffer(|buffer| ((y - start_y) / buffer.size().1));
                                terminal.scroll_to(start_scroll.0 + scroll_offset);
                            }
                        }
                    }
                    status = Status::Captured;
                }
            }
            Event::Mouse(MouseEvent::WheelScrolled { delta }) => {
                if let Some(_p) = cursor_position.position_in(layout.bounds()) {
                    match delta {
                        ScrollDelta::Lines { x, y } => {
                            //TODO: this adjustment is just a guess!
                            state.scroll_pixels = 0.0;
                            let lines = (-y * 6.0) as i32;
                            if lines != 0 {
                                terminal.scroll(TerminalScroll::Delta(-lines));
                            }
                            status = Status::Captured;
                        }
                        ScrollDelta::Pixels { x, y } => {
                            //TODO: this adjustment is just a guess!
                            state.scroll_pixels -= y * 6.0;
                            let mut lines = 0;
                            let metrics = terminal.with_buffer(|buffer| buffer.metrics());
                            while state.scroll_pixels <= -metrics.line_height {
                                lines -= 1;
                                state.scroll_pixels += metrics.line_height;
                            }
                            while state.scroll_pixels >= metrics.line_height {
                                lines += 1;
                                state.scroll_pixels -= metrics.line_height;
                            }
                            if lines != 0 {
                                terminal.scroll(TerminalScroll::Delta(-lines));
                            }
                            status = Status::Captured;
                        }
                    }
                }
            }
            _ => (),
        }

        status
    }
}

impl<'a, Message, Renderer> From<TerminalBox<'a, Message>> for Element<'a, Message, Renderer>
where
    Message: Clone + 'a,
    Renderer:
        renderer::Renderer + image::Renderer<Handle = image::Handle> + text::Renderer<Raw = Raw>,
{
    fn from(terminal_box: TerminalBox<'a, Message>) -> Self {
        Self::new(terminal_box)
    }
}

enum ClickKind {
    Single,
    Double,
    Triple,
}

enum Dragging {
    Buffer,
    Scrollbar {
        start_y: f32,
        start_scroll: (f32, f32),
    },
}

pub struct State {
    modifiers: Modifiers,
    click: Option<(ClickKind, Instant)>,
    dragging: Option<Dragging>,
    scale_factor: Cell<f32>,
    scroll_pixels: f32,
    scrollbar_rect: Cell<Rectangle<f32>>,
}

impl State {
    /// Creates a new [`State`].
    pub fn new() -> State {
        State {
            modifiers: Modifiers::empty(),
            click: None,
            dragging: None,
            scale_factor: Cell::new(1.0),
            scroll_pixels: 0.0,
            scrollbar_rect: Cell::new(Rectangle::default()),
        }
    }
}
