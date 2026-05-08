mod audio;

use std::path::PathBuf;

use assets::Assets;
use editor::Editor;
use gpui::{
    App, Application, Bounds, Context, Entity, FocusHandle, Focusable, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use language::Buffer;
use settings::{DEFAULT_KEYMAP_PATH, KeybindSource, KeymapFile};

use crate::audio::AudioEngine;

actions!(modz, [RunDsl]);

struct EditorView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    source_path: Option<PathBuf>,
}

impl EditorView {
    fn run_dsl(&mut self, _: &RunDsl, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text(cx);
        match &self.source_path {
            Some(path) => match std::fs::write(path, &text) {
                Ok(_) => eprintln!("[modz] saved {}", path.display()),
                Err(err) => eprintln!("[modz] save failed: {err}"),
            },
            None => eprintln!("[modz] cmd-s pressed (no source file)"),
        }
        eprintln!("[modz] DSL exec stub — buffer length {} bytes", text.len());
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

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            settings::init(cx);
            theme::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);

            // Load Zed's default keymap so editor actions (Backspace, Newline,
            // Cmd-Z, etc.) are wired. Allow partial failure so any actions
            // that aren't registered in this binary don't kill startup.
            match KeymapFile::load_asset_allow_partial_failure(DEFAULT_KEYMAP_PATH, cx) {
                Ok(bindings) => {
                    let mut bindings = bindings;
                    for kb in bindings.iter_mut() {
                        kb.set_meta(KeybindSource::Default.meta());
                    }
                    cx.bind_keys(bindings);
                }
                Err(err) => eprintln!("[modz] failed to load default keymap: {err}"),
            }

            // Custom Modz bindings.
            cx.bind_keys([gpui::KeyBinding::new(
                "cmd-s",
                RunDsl,
                Some("Modz"),
            )]);

            let _engine = engine; // keep stream alive for window lifetime

            let bounds = Bounds::centered(None, size(px(900.), px(640.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    let initial_text = initial.clone();
                    let source_path = cli_path.clone();
                    cx.new(|cx| {
                        let buffer = cx.new(|cx| Buffer::local(initial_text, cx));
                        let editor =
                            cx.new(|cx| Editor::for_buffer(buffer, None, window, cx));
                        let focus_handle = cx.focus_handle();
                        // Focus the editor so it receives keystrokes.
                        let editor_focus = editor.read(cx).focus_handle(cx);
                        window.focus(&editor_focus, cx);
                        EditorView {
                            editor,
                            focus_handle,
                            source_path,
                        }
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
