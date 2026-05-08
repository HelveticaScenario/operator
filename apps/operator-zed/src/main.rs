mod audio;
mod dsl;
mod dsl_runtime;
mod editor_view;
mod file_explorer;
mod root_view;

use std::path::PathBuf;

use assets::Assets;
use gpui::{
    App, Application, Bounds, Window, WindowBounds, WindowOptions, prelude::*, px, size,
};
use settings::{DEFAULT_KEYMAP_PATH, KeybindSource, KeymapFile};

use crate::audio::AudioEngine;
use crate::dsl::run_and_send_patch;
use crate::editor_view::{EditorView, RunDsl};
use crate::file_explorer::FileExplorer;
use crate::root_view::RootView;

fn print_help() {
    println!(
        "modz {} — operator-zed prototype\n\n\
         USAGE:\n    \
             modz [OPTIONS] [FILE]\n\n\
         ARGS:\n    \
             FILE    Path to a .modular DSL script\n\n\
         OPTIONS:\n    \
             --emit-graph    Run the DSL on FILE and print PatchGraph JSON to stdout\n    \
             -h, --help      Show this help",
        env!("CARGO_PKG_VERSION"),
    );
}

fn run_emit_graph(path: Option<&std::path::Path>) {
    let Some(path) = path else {
        eprintln!("--emit-graph requires a path argument");
        std::process::exit(2);
    };
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("read {}: {err}", path.display());
            std::process::exit(1);
        }
    };

    let mut runtime = match dsl_runtime::DslRuntime::new() {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("DslRuntime init failed: {err}");
            std::process::exit(1);
        }
    };
    match runtime.execute(&source) {
        Ok(value) => {
            println!("{value}");
            let exit = if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                0
            } else {
                1
            };
            std::process::exit(exit);
        }
        Err(err) => {
            eprintln!("DSL execute failed: {err}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut emit_graph = false;
    let mut positional: Vec<String> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--emit-graph" => emit_graph = true,
            "-h" | "--help" => {
                print_help();
                return;
            }
            _ => positional.push(arg),
        }
    }
    let cli_path: Option<PathBuf> = positional.into_iter().next().map(PathBuf::from);

    if emit_graph {
        run_emit_graph(cli_path.as_deref());
        return;
    }

    let initial = cli_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| "// pass a file path as argv[1]\n".to_string());

    // Pick a workspace root for the file explorer: the parent dir of the cli
    // path if there is one, otherwise the current working directory.
    let workspace_root: PathBuf = cli_path
        .as_ref()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let engine = match AudioEngine::start() {
        Ok(engine) => Some(engine),
        Err(err) => {
            eprintln!("audio engine disabled: {err}");
            None
        }
    };

    let patch_tx = engine.as_ref().map(|e| e.patch_tx.clone());
    let sample_rate = engine.as_ref().map(|e| e.sample_rate).unwrap_or(48_000.0);

    if !initial.trim().is_empty() {
        if let Err(err) = run_and_send_patch(&initial, sample_rate, patch_tx.as_ref()) {
            eprintln!("[modz] startup DSL run: {err}");
        }
    }

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

            cx.bind_keys([gpui::KeyBinding::new(
                "cmd-s",
                RunDsl,
                Some("Modz"),
            )]);

            let _engine = engine;

            let bounds = Bounds::centered(None, size(px(1100.), px(700.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window: &mut Window, cx| {
                    let initial_text = initial.clone();
                    let source_path = cli_path.clone();
                    let patch_tx = patch_tx.clone();
                    let workspace_root = workspace_root.clone();
                    cx.new(|cx| {
                        let editor_view = cx.new(|cx| {
                            EditorView::new(
                                initial_text,
                                source_path,
                                patch_tx,
                                sample_rate,
                                window,
                                cx,
                            )
                        });
                        let explorer = cx.new(|cx| {
                            FileExplorer::new(&workspace_root, editor_view.clone(), cx)
                        });
                        RootView {
                            explorer,
                            editor_view,
                        }
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
