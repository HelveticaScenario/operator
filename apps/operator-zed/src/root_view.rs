//! Top-level window content: file explorer | editor stack with the controls
//! panel underneath the editor.

use gpui::{Context, Entity, Render, Window, div, prelude::*};

use crate::controls::ControlsView;
use crate::editor_view::EditorView;
use crate::file_explorer::FileExplorer;
use crate::scopes::ScopesView;

pub struct RootView {
    pub explorer: Entity<FileExplorer>,
    pub editor_view: Entity<EditorView>,
    pub controls: Entity<ControlsView>,
    pub scopes: Entity<ScopesView>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .size_full()
            .child(self.explorer.clone())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .size_full()
                    .child(self.editor_view.clone())
                    .child(self.scopes.clone())
                    .child(self.controls.clone()),
            )
    }
}
