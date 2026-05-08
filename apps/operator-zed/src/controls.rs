//! Controls panel: one row per `$slider()` declared by the DSL.
//!
//! Each row is a horizontal rail; click-and-drag along the rail sets the
//! slider value. The drag handler writes the new value through
//! `DslState::set_slider_value`, which mutates the cached graph JSON,
//! rebuilds a `Patch`, and sends it to the audio thread — no JS round-trip.

use gpui::{
    Context, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Render, Window, div,
    prelude::*, px, rgb,
};

use crate::dsl_state::{DslState, SliderDef};
use crate::editor_view::EditorView;

/// Width of the slider rail in pixels.
const RAIL_WIDTH: f32 = 220.0;

#[derive(Default)]
struct DragState {
    /// Index of the slider currently being dragged (in `state.sliders`).
    active: Option<usize>,
    /// Most recent pointer x position so we can apply incremental deltas.
    last_x: Pixels,
}

pub struct ControlsView {
    state: Entity<DslState>,
    editor_view: Entity<EditorView>,
    drag: DragState,
}

impl ControlsView {
    pub fn new(
        state: Entity<DslState>,
        editor_view: Entity<EditorView>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state,
            editor_view,
            drag: DragState::default(),
        }
    }

    fn handle_down(
        &mut self,
        ix: usize,
        event: &MouseDownEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        self.drag.active = Some(ix);
        self.drag.last_x = event.position.x;
    }

    fn handle_move(
        &mut self,
        ix: usize,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            // Reset drag state so a stray move event doesn't keep dragging
            // after the user released outside the rail.
            if self.drag.active.is_some() {
                self.drag.active = None;
            }
            return;
        }
        if self.drag.active != Some(ix) {
            return;
        }
        let dx_px = f32::from(event.position.x) - f32::from(self.drag.last_x);
        self.drag.last_x = event.position.x;
        let Some(slider) = self.state.read(cx).sliders.get(ix).cloned() else {
            return;
        };
        let span = slider.max - slider.min;
        if span <= 0.0 {
            return;
        }
        let delta = (dx_px as f64 / RAIL_WIDTH as f64) * span;
        if delta.abs() < f64::EPSILON {
            return;
        }
        let label = slider.label.clone();
        let new_value = self.state.update(cx, |state, cx| {
            state.bump_slider(&label, delta, _window, cx)
        });
        if let Some(value) = new_value {
            // Mirror the change into the source code so cmd-S preserves it.
            let editor_view = self.editor_view.clone();
            let label_for_edit = label.clone();
            editor_view.update(cx, |view, cx| {
                view.rewrite_slider_call(&label_for_edit, value, cx);
            });
        }
    }

    fn handle_up(
        &mut self,
        _event: &gpui::MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.drag.active = None;
    }
}

impl Render for ControlsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sliders: Vec<SliderDef> = self.state.read(cx).sliders.clone();

        let mut container = div()
            .w_full()
            .min_h(px(36.))
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(rgb(0x2a2c2e))
            .bg(rgb(0x1a1c1e))
            .text_color(rgb(0xc0c2c4))
            .flex()
            .flex_col()
            .gap_1()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event, window, cx| this.handle_up(event, window, cx)),
            );

        if sliders.is_empty() {
            container = container.child(
                div()
                    .text_color(rgb(0x70737a))
                    .child("(no sliders — declare with $slider('Label', value, min, max))"),
            );
            return container;
        }

        for (ix, slider) in sliders.iter().enumerate() {
            let span = (slider.max - slider.min).max(f64::EPSILON);
            let position = ((slider.value - slider.min) / span).clamp(0.0, 1.0) as f32;
            let thumb_x = position * RAIL_WIDTH;

            container = container.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(140.))
                            .text_color(rgb(0xe8c468))
                            .child(slider.label.clone()),
                    )
                    .child(
                        // Rail
                        div()
                            .id(("modz-slider", ix))
                            .relative()
                            .w(px(RAIL_WIDTH))
                            .h(px(20.))
                            .bg(rgb(0x252729))
                            .border_1()
                            .border_color(rgb(0x3a3c3e))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event, window, cx| {
                                    this.handle_down(ix, event, window, cx)
                                }),
                            )
                            .on_mouse_move(cx.listener(move |this, event, window, cx| {
                                this.handle_move(ix, event, window, cx)
                            }))
                            .child(
                                // Filled track
                                div()
                                    .absolute()
                                    .left(px(0.))
                                    .top(px(0.))
                                    .w(px(thumb_x))
                                    .h(px(20.))
                                    .bg(rgb(0x3a5470)),
                            )
                            .child(
                                // Thumb
                                div()
                                    .absolute()
                                    .left(px((thumb_x - 3.).max(0.)))
                                    .top(px(0.))
                                    .w(px(6.))
                                    .h(px(20.))
                                    .bg(rgb(0xc0c2c4)),
                            ),
                    )
                    .child(
                        div()
                            .min_w(px(60.))
                            .text_align(gpui::TextAlign::Right)
                            .child(format_value(slider.value)),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x70737a))
                            .child(format!(
                                "{} – {}",
                                format_value(slider.min),
                                format_value(slider.max)
                            )),
                    ),
            );
        }

        container
    }
}

fn format_value(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.3}")
    }
}
