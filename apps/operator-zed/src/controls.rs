//! Controls panel: one row per `$slider()` declared by the DSL.
//!
//! Each row shows the label + current value + min/max, and a pair of step
//! buttons that bump the value through `DslState::bump_slider`. That writes
//! the new value back into the cached graph JSON, rebuilds a `Patch`, and
//! pushes it to the audio thread — no JS re-execution.

use gpui::{Context, Entity, Render, Window, div, prelude::*, px, rgb};

use crate::dsl_state::{DslState, SliderDef};

pub struct ControlsView {
    state: Entity<DslState>,
}

impl ControlsView {
    pub fn new(state: Entity<DslState>, _cx: &mut Context<Self>) -> Self {
        Self { state }
    }
}

impl Render for ControlsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sliders: Vec<SliderDef> = self.state.read(cx).sliders.clone();
        let state = self.state.clone();

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
            .gap_2();

        if sliders.is_empty() {
            container = container.child(
                div()
                    .text_color(rgb(0x70737a))
                    .child("(no sliders — declare with $slider('Label', value, min, max))"),
            );
            return container;
        }

        for (ix, slider) in sliders.iter().enumerate() {
            let label = slider.label.clone();
            let dec_label = label.clone();
            let inc_label = label.clone();
            let state_dec = state.clone();
            let state_inc = state.clone();
            let step = (slider.max - slider.min) * 0.05;
            let value_text = format_value(slider.value);
            let range_text = format!(
                "{} – {}",
                format_value(slider.min),
                format_value(slider.max)
            );

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
                        div()
                            .id(("modz-slider-dec", ix))
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x3a3c3e))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x252729)))
                            .on_click(move |_, window, cx| {
                                let l = dec_label.clone();
                                state_dec.update(cx, |s, cx| {
                                    s.bump_slider(&l, -step, window, cx);
                                });
                            })
                            .child("−"),
                    )
                    .child(
                        div()
                            .min_w(px(80.))
                            .text_align(gpui::TextAlign::Center)
                            .child(value_text),
                    )
                    .child(
                        div()
                            .id(("modz-slider-inc", ix))
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x3a3c3e))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x252729)))
                            .on_click(move |_, window, cx| {
                                let l = inc_label.clone();
                                state_inc.update(cx, |s, cx| {
                                    s.bump_slider(&l, step, window, cx);
                                });
                            })
                            .child("+"),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x70737a))
                            .child(range_text),
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
