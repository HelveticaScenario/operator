//! Top-level window content: file explorer | editor stack with the controls
//! panel underneath the editor.

use gpui::{Context, Entity, Render, Window, div, prelude::*};

use crate::controls::ControlsView;
use crate::editor_view::EditorView;
use crate::file_explorer::FileExplorer;
use crate::toolbar::Toolbar;

pub struct RootView {
    pub toolbar: Entity<Toolbar>,
    pub explorer: Entity<FileExplorer>,
    pub editor_view: Entity<EditorView>,
    pub controls: Entity<ControlsView>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.toolbar.clone())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_grow()
                    .child(self.explorer.clone())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .child(self.editor_view.clone())
                            .child(self.controls.clone()),
                    ),
            )
    }
}
