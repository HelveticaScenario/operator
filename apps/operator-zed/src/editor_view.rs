//! The editor pane: holds a Zed `Editor`, listens for cmd-S, runs the DSL,
//! and pushes the resulting Patch to the audio thread.

use std::path::{Path, PathBuf};

use crossbeam_channel::Sender;
use editor::Editor;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Window, actions, div, prelude::*, rgb,
};
use language::Buffer;
use modular_core::patch::Patch;

use crate::dsl::run_and_send_patch;

actions!(modz, [RunDsl]);

pub struct EditorView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    source_path: Option<PathBuf>,
    patch_tx: Option<Sender<Patch>>,
    sample_rate: f32,
}

impl EditorView {
    pub fn new(
        initial_text: String,
        source_path: Option<PathBuf>,
        patch_tx: Option<Sender<Patch>>,
        sample_rate: f32,
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
            patch_tx,
            sample_rate,
        }
    }

    /// Replace the editor buffer with the contents of `path`, update the save
    /// target, and re-run the DSL so the audio thread reflects the new file.
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
            // The Editor wraps a single Buffer in a multi-buffer; replace the
            // text on that underlying buffer.
            if let Some(handle) = multi.all_buffers().into_iter().next() {
                handle.update(cx, |buffer, cx| {
                    buffer.set_text(text.clone(), cx);
                });
            }
        });
        self.source_path = Some(path.to_path_buf());

        if let Err(err) = run_and_send_patch(&text, self.sample_rate, self.patch_tx.as_ref())
        {
            eprintln!("[modz] open-file DSL run: {err}");
        }
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
        if let Err(err) = run_and_send_patch(&text, self.sample_rate, self.patch_tx.as_ref())
        {
            eprintln!("[modz] {err}");
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
