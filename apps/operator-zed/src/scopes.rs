//! Scopes panel: one row per scope channel, drawn as a unicode-block
//! waveform sampled from the audio thread's ring buffer.
//!
//! Production scope rendering lives inline in the editor as a gpui block
//! decoration — see HANDOFF.md Step 6 for the full plan with
//! `BlockProperties::Below(anchor)`. The prototype here uses a row-per-channel
//! panel so the data path (audio -> ring -> UI) is exercised end-to-end
//! before the inline-block rendering work lands.

use std::sync::Arc;

use gpui::{Context, Entity, Render, Window, div, prelude::*, px, rgb};
use parking_lot::Mutex;

use crate::dsl_state::{DslState, ScopeTarget};

/// How frequently the panel polls the rings, in milliseconds.
const POLL_INTERVAL_MS: u64 = 33; // ~30 Hz

pub struct ScopesView {
    state: Entity<DslState>,
}

impl ScopesView {
    pub fn new(state: Entity<DslState>, cx: &mut Context<Self>) -> Self {
        // Re-render at a fixed cadence so the waveform updates while audio
        // pushes new samples into the rings.
        let interval = std::time::Duration::from_millis(POLL_INTERVAL_MS);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if this
                    .update(cx, |_this, cx| {
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self { state }
    }
}

impl Render for ScopesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let targets: Arc<Mutex<Vec<ScopeTarget>>> = self.state.read(cx).scope_targets.clone();
        let snapshot: Vec<ScopeTarget> = targets.lock().clone();

        let mut container = div()
            .w_full()
            .min_h(px(28.))
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(0x2a2c2e))
            .bg(rgb(0x141618))
            .text_color(rgb(0xa0a2a4))
            .flex()
            .flex_col()
            .gap_1();

        if snapshot.is_empty() {
            container = container.child(
                div()
                    .text_color(rgb(0x70737a))
                    .child("(no scopes — call `.scope()` on a signal)"),
            );
            return container;
        }

        for target in snapshot.iter() {
            let waveform = render_waveform(target);
            container = container.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .min_w(px(180.))
                            .text_color(rgb(0x88c4f8))
                            .child(target.label.clone()),
                    )
                    .child(
                        div()
                            .text_color(rgb(0xc8caf0))
                            .child(waveform),
                    ),
            );
        }
        container
    }
}

/// Render the most recent ~80 samples of the ring as 8-level unicode block
/// glyphs ("▁▂▃▄▅▆▇█"), normalized into the scope's voltage range.
fn render_waveform(target: &ScopeTarget) -> String {
    const WIDTH: usize = 80;
    let glyphs = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let ring = target.samples.lock();
    if ring.is_empty() {
        return "(silence)".to_string();
    }
    // Take a uniformly-spaced sample of WIDTH points across the buffer.
    let len = ring.len();
    let mut out = String::with_capacity(WIDTH);
    for i in 0..WIDTH {
        let ix = (i * len) / WIDTH;
        let v = ring[ix];
        let (lo, hi) = target.range;
        let span = (hi - lo).max(f64::EPSILON);
        let norm = (((v as f64) - lo) / span).clamp(0.0, 1.0);
        let bin = (norm * (glyphs.len() - 1) as f64).round() as usize;
        out.push(glyphs[bin]);
    }
    out
}
