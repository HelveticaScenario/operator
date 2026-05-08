//! Simple file-explorer panel for the operator-zed shell.
//!
//! Walks the workspace root once at startup, lists files with a
//! `.modular` extension at the top, then everything else. Clicking a row
//! tells the `EditorView` to swap its buffer.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Render, Window, div, prelude::*, px, rgb,
    uniform_list,
};

use crate::editor_view::EditorView;

#[derive(Clone)]
struct FileEntry {
    label: String,
    path: PathBuf,
    is_modular: bool,
}

pub struct FileExplorer {
    entries: Vec<FileEntry>,
    editor_view: Entity<EditorView>,
    focus_handle: FocusHandle,
}

impl FileExplorer {
    pub fn new(
        root: &Path,
        editor_view: Entity<EditorView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let entries = list_files(root);
        Self {
            entries,
            editor_view,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for FileExplorer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FileExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.entries.clone();
        let editor_view = self.editor_view.clone();
        let total = entries.len();
        div()
            .track_focus(&self.focus_handle)
            .key_context("ModzExplorer")
            .h_full()
            .w(px(220.))
            .bg(rgb(0x16181a))
            .border_r_1()
            .border_color(rgb(0x2a2c2e))
            .text_color(rgb(0xc0c2c4))
            .child(
                uniform_list(
                    "modz-files",
                    total,
                    cx.processor({
                        let entries = entries.clone();
                        let editor_view = editor_view.clone();
                        move |_this, range, _window, _cx| {
                            let mut rows = Vec::new();
                            for ix in range {
                                let Some(entry) = entries.get(ix as usize) else {
                                    continue;
                                };
                                let path = entry.path.clone();
                                let editor_view = editor_view.clone();
                                let label_color = if entry.is_modular {
                                    rgb(0xe8c468)
                                } else {
                                    rgb(0xa0a2a4)
                                };
                                rows.push(
                                    div()
                                        .id(("modz-file", ix))
                                        .px_2()
                                        .py_1()
                                        .cursor_pointer()
                                        .text_color(label_color)
                                        .hover(|s| s.bg(rgb(0x202224)))
                                        .on_click(move |_event, window, cx| {
                                            let path = path.clone();
                                            editor_view.update(cx, |view, cx| {
                                                view.open_file(&path, window, cx);
                                            });
                                        })
                                        .child(entry.label.clone()),
                                );
                            }
                            rows
                        }
                    }),
                )
                .h_full(),
            )
    }
}

fn list_files(root: &Path) -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = Vec::new();
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("[modz/explorer] read_dir({}): {err}", root.display());
            return entries;
        }
    };
    for dirent in read.flatten() {
        let path = dirent.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let is_dir = dirent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            // Shallow listing for the prototype — leave directory recursion
            // to a later pass.
            continue;
        }
        let is_modular = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("modular"));
        entries.push(FileEntry {
            label: name.to_string(),
            path,
            is_modular,
        });
    }
    entries.sort_by(|a, b| match (a.is_modular, b.is_modular) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
    });
    entries
}
