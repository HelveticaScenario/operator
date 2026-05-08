mod audio;
mod controls;
mod dsl;
mod dsl_runtime;
mod dsl_state;
mod editor_view;
mod file_explorer;
mod root_view;
mod scopes;

use std::path::PathBuf;

use assets::Assets;
use gpui::{
    App, Application, Bounds, Window, WindowBounds, WindowOptions, prelude::*, px, size,
};
use settings::{DEFAULT_KEYMAP_PATH, KeybindSource, KeymapFile};

use crate::audio::AudioEngine;
use crate::controls::ControlsView;
use crate::dsl_state::DslState;
use crate::editor_view::{EditorView, RunDsl};
use crate::file_explorer::FileExplorer;
use crate::root_view::RootView;
use crate::scopes::ScopesView;

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
    let scope_targets = engine
        .as_ref()
        .map(|e| e.scope_targets.clone())
        .unwrap_or_else(|| std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())));

    // Run the startup DSL once, but stash the result for the gpui side to
    // copy into DslState. This way the panels come up populated.
    let startup_execution = if !initial.trim().is_empty() {
        match dsl::run(&initial, sample_rate) {
            Ok(execution) => {
                if let Some(tx) = patch_tx.as_ref() {
                    if let Err(err) = tx.try_send(execution.patch) {
                        eprintln!("[modz] startup audio send: {err}");
                        None
                    } else {
                        eprintln!(
                            "[modz] DSL ok — {} modules, {} sliders, {} scopes (startup)",
                            execution.module_count,
                            execution.sliders.len(),
                            execution.scopes.len(),
                        );
                        // Seed audio's scope targets immediately so the cpal
                        // callback starts pushing samples on the next frame.
                        *scope_targets.lock() = execution.scopes.clone();
                        Some((
                            execution.graph_value,
                            execution.sliders,
                            execution.scopes,
                        ))
                    }
                } else {
                    Some((execution.graph_value, execution.sliders, execution.scopes))
                }
            }
            Err(err) => {
                eprintln!("[modz] startup DSL run: {err}");
                None
            }
        }
    } else {
        None
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
                    let startup_execution = startup_execution.clone();
                    cx.new(|cx| {
                        let state = cx.new(|_cx| {
                            let mut state = DslState::new(
                                sample_rate,
                                patch_tx,
                                scope_targets.clone(),
                            );
                            if let Some((graph_value, sliders, _scopes)) =
                                startup_execution
                            {
                                state.sliders = sliders;
                                state.set_graph_value(graph_value);
                            }
                            state
                        });
                        let editor_view = cx.new(|cx| {
                            EditorView::new(
                                initial_text,
                                source_path,
                                state.clone(),
                                window,
                                cx,
                            )
                        });
                        let explorer = cx.new(|cx| {
                            FileExplorer::new(&workspace_root, editor_view.clone(), cx)
                        });
                        let controls =
                            cx.new(|cx| ControlsView::new(state.clone(), cx));
                        let scopes_view =
                            cx.new(|cx| ScopesView::new(state.clone(), cx));
                        RootView {
                            explorer,
                            editor_view,
                            controls,
                            scopes: scopes_view,
                        }
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
