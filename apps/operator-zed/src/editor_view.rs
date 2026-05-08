//! The editor pane: holds a Zed `Editor`, listens for cmd-S, runs the DSL,
//! pushes the resulting `Patch` to the audio thread, and updates the shared
//! `DslState` (graph + sliders) so the controls panel re-renders.

use std::path::{Path, PathBuf};

use editor::Editor;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Window, actions, div, prelude::*, rgb,
};
use language::Buffer;

use crate::dsl;
use crate::dsl_state::DslState;

actions!(modz, [RunDsl]);

pub struct EditorView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    source_path: Option<PathBuf>,
    state: Entity<DslState>,
}

impl EditorView {
    pub fn new(
        initial_text: String,
        source_path: Option<PathBuf>,
        state: Entity<DslState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| Buffer::local(initial_text, cx));
        let editor = cx.new(|cx| Editor::for_buffer(buffer, None, window, cx));
        let focus_handle = cx.focus_handle();
        let editor_focus = editor.read(cx).focus_handle(cx);
        window.focus(&editor_focus, cx);
        Self {
            editor,
            focus_handle,
            source_path,
            state,
        }
    }

    /// Replace the editor buffer with the contents of `path`, update the save
    /// target, and re-run the DSL so audio + sliders track the new file.
    pub fn open_file(&mut self, path: &Path, _window: &mut Window, cx: &mut Context<Self>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[modz] open {}: {err}", path.display());
                return;
            }
        };
        let multi = self.editor.read(cx).buffer().clone();
        multi.update(cx, |multi, cx| {
            if let Some(handle) = multi.all_buffers().into_iter().next() {
                handle.update(cx, |buffer, cx| {
                    buffer.set_text(text.clone(), cx);
                });
            }
        });
        self.source_path = Some(path.to_path_buf());
        self.execute(&text, cx);
        cx.notify();
    }

    fn run_dsl(&mut self, _: &RunDsl, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text(cx);
        match &self.source_path {
            Some(path) => match std::fs::write(path, &text) {
                Ok(_) => eprintln!("[modz] saved {}", path.display()),
                Err(err) => eprintln!("[modz] save failed: {err}"),
            },
            None => eprintln!("[modz] cmd-s pressed (no source file)"),
        }
        self.execute(&text, cx);
    }

    fn execute(&mut self, source: &str, cx: &mut Context<Self>) {
        let sample_rate = self.state.read(cx).sample_rate();
        match dsl::run(source, sample_rate) {
            Ok(execution) => {
                if let Some(tx) = self.state.read(cx).patch_tx().cloned() {
                    if let Err(err) = tx.try_send(execution.patch) {
                        eprintln!("[modz] audio channel send: {err}");
                    }
                }
                let module_count = execution.module_count;
                let slider_count = execution.sliders.len();
                self.state.update(cx, |state, cx| {
                    state.update_after_exec(execution.graph_value, execution.sliders, cx);
                });
                eprintln!(
                    "[modz] DSL ok — {module_count} modules, {slider_count} sliders"
                );
            }
            Err(err) => eprintln!("[modz] {err}"),
        }
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .key_context("Modz")
            .on_action(cx.listener(Self::run_dsl))
            .flex()
            .flex_grow()
            .size_full()
            .bg(rgb(0x1d1f21))
            .child(self.editor.clone())
    }
}
