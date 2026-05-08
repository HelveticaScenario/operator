//! File-explorer panel: recursive directory tree under a workspace root.
//!
//! Click a file to swap the EditorView buffer. Click a directory to
//! toggle expansion. `.modular` files sort first within each directory.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Render, Window, div, prelude::*, px, rgb,
};

use crate::editor_view::EditorView;

#[derive(Clone)]
struct FileEntry {
    label: String,
    path: PathBuf,
    is_dir: bool,
    is_modular: bool,
    /// Depth in the tree (0 = direct child of root).
    depth: usize,
}

pub struct FileExplorer {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    editor_view: Entity<EditorView>,
    focus_handle: FocusHandle,
}

impl FileExplorer {
    pub fn new(
        root: &Path,
        editor_view: Entity<EditorView>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            root: root.to_path_buf(),
            expanded: HashSet::new(),
            editor_view,
            focus_handle: cx.focus_handle(),
        }
    }

    fn flatten(&self) -> Vec<FileEntry> {
        let mut out = Vec::new();
        walk(&self.root, 0, &self.expanded, &mut out);
        out
    }

    fn toggle(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        cx.notify();
    }
}

impl Focusable for FileExplorer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FileExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.flatten();
        let editor_view = self.editor_view.clone();

        let mut container = div()
            .id("modz-explorer-root")
            .track_focus(&self.focus_handle)
            .key_context("ModzExplorer")
            .h_full()
            .w(px(240.))
            .min_w(px(240.))
            .bg(rgb(0x16181a))
            .border_r_1()
            .border_color(rgb(0x2a2c2e))
            .text_color(rgb(0xc0c2c4))
            .overflow_y_scroll();

        for (ix, entry) in entries.into_iter().enumerate() {
            let path = entry.path.clone();
            let is_dir = entry.is_dir;
            let editor_view = editor_view.clone();
            let label_color = if entry.is_modular {
                rgb(0xe8c468)
            } else if entry.is_dir {
                rgb(0xc8caf0)
            } else {
                rgb(0xa0a2a4)
            };
            let prefix = if entry.is_dir {
                let expanded = self.expanded.contains(&entry.path);
                if expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let indent_px = (entry.depth as f32) * 12.0;
            let display = format!("{prefix}{}", entry.label);

            container = container.child(
                div()
                    .id(("modz-file", ix))
                    .px_2()
                    .py(px(2.))
                    .pl(px(8. + indent_px))
                    .text_color(label_color)
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x202224)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if is_dir {
                            this.toggle(path.clone(), cx);
                        } else {
                            let path = path.clone();
                            editor_view.update(cx, |view, cx| {
                                view.open_file(&path, window, cx);
                            });
                        }
                    }))
                    .child(display),
            );
        }
        container
    }
}

fn walk(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    out: &mut Vec<FileEntry>,
) {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("[modz/explorer] read_dir({}): {err}", dir.display());
            return;
        }
    };
    let mut entries: Vec<FileEntry> = Vec::new();
    for dirent in read.flatten() {
        let path = dirent.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let is_dir = dirent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let is_modular = !is_dir
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("modular"));
        entries.push(FileEntry {
            label: name.to_string(),
            path,
            is_dir,
            is_modular,
            depth,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => match (a.is_modular, b.is_modular) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
        },
    });
    for entry in entries {
        let is_dir = entry.is_dir;
        let path = entry.path.clone();
        out.push(entry);
        if is_dir && expanded.contains(&path) {
            walk(&path, depth + 1, expanded, out);
        }
    }
}
