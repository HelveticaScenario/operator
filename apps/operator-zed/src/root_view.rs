//! Top-level window content: file explorer on the left, editor on the right.

use gpui::{Context, Entity, Render, Window, div, prelude::*};

use crate::editor_view::EditorView;
use crate::file_explorer::FileExplorer;

pub struct RootView {
    pub explorer: Entity<FileExplorer>,
    pub editor_view: Entity<EditorView>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .size_full()
            .child(self.explorer.clone())
            .child(self.editor_view.clone())
    }
}
