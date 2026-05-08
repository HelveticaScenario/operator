mod audio;

use std::path::PathBuf;

use assets::Assets;
use editor::Editor;
use gpui::{
    App, Application, Bounds, Context, Entity, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use language::Buffer;

use crate::audio::AudioEngine;

struct EditorView {
    editor: Entity<Editor>,
}

impl gpui::Render for EditorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(0x1d1f21))
            .child(self.editor.clone())
    }
}

fn main() {
    let cli_path: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);
    let initial = cli_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| "// pass a file path as argv[1]\n".to_string());

    let engine = match AudioEngine::start() {
        Ok(engine) => Some(engine),
        Err(err) => {
            eprintln!("audio engine disabled: {err}");
            None
        }
    };

    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        settings::init(cx);
        theme::init(theme::LoadThemes::JustBase, cx);

        let _engine = engine; // keep stream alive for window lifetime

        let bounds = Bounds::centered(None, size(px(900.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let initial_text = initial.clone();
                cx.new(|cx| {
                    let buffer = cx.new(|cx| Buffer::local(initial_text, cx));
                    let editor =
                        cx.new(|cx| Editor::for_buffer(buffer, None, window, cx));
                    EditorView { editor }
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
