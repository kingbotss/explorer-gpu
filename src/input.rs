use explorer::text::Text;
use gpui::{prelude::*, *};
use std::ops::Range;
#[derive(Clone, Debug)]
pub enum InputEvent {
    Changed,
    Submit,
    Cancel,
}
pub struct Input {
    pub text: Text,
    pub focus: FocusHandle,
    placeholder: SharedString,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll: Pixels,
    dragging: bool,
}
impl EventEmitter<InputEvent> for Input {}
impl Input {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            text: Text::default(),
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            layout: None,
            bounds: None,
            scroll: px(0.),
            dragging: false,
        }
    }
    pub fn value(&self) -> &str {
        &self.text.value
    }
    pub fn set(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.text = Text::new(value);
        self.scroll = px(0.);
        cx.notify();
    }
    pub fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text.select_all();
        self.focus.focus(window);
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Changed);
        cx.notify();
    }
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        if m.platform {
            match k.key.as_str() {
                "a" => self.text.select_all(),
                "c" | "x" => {
                    let r = self.text.selection();
                    if !r.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            self.text.value[r].to_owned(),
                        ));
                        if k.key == "x" {
                            self.text.replace(None, "");
                            self.changed(cx);
                        }
                    }
                }
                "v" => {
                    if let Some(s) = cx.read_from_clipboard().and_then(|i| i.text()) {
                        self.text.replace(None, &s);
                        self.changed(cx);
                    }
                }
                "left" => {
                    self.text.cursor = 0;
                    if !m.shift {
                        self.text.anchor = 0;
                    }
                }
                "right" => {
                    self.text.cursor = self.text.value.len();
                    if !m.shift {
                        self.text.anchor = self.text.cursor;
                    }
                }
                _ => return,
            }
        } else {
            match k.key.as_str() {
                "enter" => {
                    cx.emit(InputEvent::Submit);
                }
                "escape" => {
                    cx.emit(InputEvent::Cancel);
                }
                "backspace" => {
                    self.text.backspace();
                    self.changed(cx);
                }
                "delete" => {
                    self.text.delete();
                    self.changed(cx);
                }
                "left" => self.text.left(m.shift),
                "right" => self.text.right(m.shift),
                "home" => {
                    self.text.cursor = 0;
                    if !m.shift {
                        self.text.anchor = 0;
                    }
                }
                "end" => {
                    self.text.cursor = self.text.value.len();
                    if !m.shift {
                        self.text.anchor = self.text.cursor;
                    }
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        let _ = window;
        cx.notify();
    }
    fn index(&self, point: Point<Pixels>) -> usize {
        match (&self.layout, self.bounds) {
            (Some(line), Some(b)) => self
                .text
                .boundary(line.closest_index_for_x(point.x - b.left() + self.scroll)),
            _ => self.text.value.len(),
        }
    }
}
impl Focusable for Input {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.text.from_utf16(r);
        *adjusted = Some(self.text.to_utf16(r.clone()));
        Some(self.text.value[r].to_owned())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.text.to_utf16(self.text.selection()),
            reversed: self.text.cursor < self.text.anchor,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.text.marked.clone().map(|r| self.text.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.text.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        value: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = r.map(|r| self.text.from_utf16(r));
        self.text.replace(r, value);
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        value: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = r
            .map(|r| self.text.from_utf16(r))
            .or(self.text.marked.clone())
            .unwrap_or_else(|| self.text.selection());
        let start = r.start;
        self.text.replace(Some(r), value);
        self.text.marked = (!value.is_empty()).then_some(start..self.text.cursor);
        if let Some(s) = selected {
            let fragment = Text::new(value);
            self.text.anchor = start + fragment.utf8(s.start);
            self.text.cursor = start + fragment.utf8(s.end);
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.layout.as_ref()?;
        let r = self.text.from_utf16(r);
        Some(Bounds::from_corners(
            point(
                bounds.left() + line.x_for_index(r.start) - self.scroll,
                bounds.top(),
            ),
            point(
                bounds.left() + line.x_for_index(r.end) - self.scroll,
                bounds.bottom(),
            ),
        ))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.text.utf16(self.index(p)))
    }
}
impl Render for Input {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let painter = entity.clone();
        let focused = self.focus.is_focused(window);
        div()
            .id("input")
            .track_focus(&self.focus)
            .flex()
            .items_center()
            .w_full()
            .h(px(32.))
            .px_2()
            .rounded_md()
            .cursor(CursorStyle::IBeam)
            .overflow_hidden()
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.focus.focus(window);
                    let index = this.index(e.position);
                    this.text.cursor = index;
                    if !e.modifiers.shift {
                        this.text.anchor = index;
                    }
                    if e.click_count >= 2 {
                        this.text.select_all();
                    }
                    this.dragging = true;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.dragging {
                    this.text.cursor = this.index(e.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let input = entity.read(cx);
                        let empty = input.text.value.is_empty();
                        let text: SharedString = if empty {
                            input.placeholder.clone()
                        } else {
                            input.text.value.clone().into()
                        };
                        let style = window.text_style();
                        let run = TextRun {
                            len: text.len(),
                            font: style.font(),
                            color: if empty {
                                rgb(0x8a929e).into()
                            } else {
                                style.color
                            },
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(text, px(13.), &[run], None);
                        let cursor = line.x_for_index(input.text.cursor);
                        let width = bounds.size.width - px(4.);
                        let mut offset = input.scroll;
                        if cursor - offset > width {
                            offset = cursor - width;
                        }
                        if cursor < offset {
                            offset = cursor;
                        }
                        if empty {
                            offset = px(0.);
                        }
                        (
                            line,
                            offset,
                            input.text.selection(),
                            input.text.marked.clone(),
                        )
                    },
                    move |bounds, (line, offset, selected, marked), window, cx| {
                        let focus = painter.read(cx).focus.clone();
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, painter.clone()),
                            cx,
                        );
                        let origin = point(bounds.left() - offset, bounds.top());
                        if focused && !selected.is_empty() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(selected.start),
                                        bounds.top(),
                                    ),
                                    size(
                                        line.x_for_index(selected.end)
                                            - line.x_for_index(selected.start),
                                        bounds.size.height,
                                    ),
                                ),
                                rgba(0x367cf84d),
                            ));
                        }
                        let _ = line.paint(origin, px(20.), window, cx);
                        if focused && selected.is_empty() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(selected.start),
                                        bounds.top() + px(2.),
                                    ),
                                    size(px(1.5), px(16.)),
                                ),
                                rgb(0x448aff),
                            ));
                        }
                        if let Some(m) = marked {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(m.start),
                                        bounds.bottom() - px(1.),
                                    ),
                                    size(
                                        line.x_for_index(m.end) - line.x_for_index(m.start),
                                        px(1.),
                                    ),
                                ),
                                rgb(0x448aff),
                            ));
                        }
                        painter.update(cx, |input, _| {
                            input.layout = Some(line);
                            input.bounds = Some(bounds);
                            input.scroll = offset;
                        });
                    },
                )
                .w_full()
                .h(px(20.)),
            )
    }
}
