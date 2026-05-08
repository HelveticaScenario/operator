//! Top toolbar: tempo / time signature read from the DSL graph, plus the
//! "Update Patch" and "Stop" buttons that mirror the production app's
//! transport controls.

use std::sync::Arc;

use gpui::{Context, Entity, MouseButton, Render, Window, div, prelude::*, px, rgb};
use parking_lot::Mutex;

use crate::audio::EngineState;
use crate::dsl_state::DslState;
use crate::editor_view::EditorView;

pub struct Toolbar {
    state: Entity<DslState>,
    editor_view: Entity<EditorView>,
    engine_state: Arc<Mutex<EngineState>>,
}

impl Toolbar {
    pub fn new(
        state: Entity<DslState>,
        editor_view: Entity<EditorView>,
        engine_state: Arc<Mutex<EngineState>>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state,
            editor_view,
            engine_state,
        }
    }
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dsl = self.state.read(cx);
        let (tempo, num, den) = dsl.clock_info();
        let muted = self.engine_state.lock().muted;

        let editor_view = self.editor_view.clone();
        let engine_state = self.engine_state.clone();
        let stop_label = if muted { "▶ Resume" } else { "■ Stop" };
        let stop_color = if muted { rgb(0x66cc7a) } else { rgb(0xe06060) };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_4()
            .px_4()
            .h(px(36.))
            .border_b_1()
            .border_color(rgb(0x2a2c2e))
            .bg(rgb(0x141618))
            .text_color(rgb(0xc0c2c4))
            .child(
                div()
                    .text_color(rgb(0xc0c2c4))
                    .min_w(px(48.))
                    .child(format!("{tempo:.0}")),
            )
            .child(
                div()
                    .text_color(rgb(0x70737a))
                    .min_w(px(40.))
                    .child(format!("{num}/{den}")),
            )
            .child(div().flex_grow())
            .child(
                div()
                    .id("modz-toolbar-update")
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x3a8c5a))
                    .text_color(rgb(0x66cc7a))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x202924)))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        editor_view.update(cx, |view, cx| view.trigger_run_dsl(window, cx));
                    })
                    .child("▶ Update Patch"),
            )
            .child(
                div()
                    .id("modz-toolbar-stop")
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x3a3c3e))
                    .text_color(stop_color)
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x252729)))
                    .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                        let mut s = engine_state.lock();
                        s.muted = !s.muted;
                        cx.refresh_windows();
                    })
                    .child(stop_label),
            )
    }
}
