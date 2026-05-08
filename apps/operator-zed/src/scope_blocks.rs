//! Inline scope overlays — rendered as `BlockProperties::Below` decorations in
//! the Zed `Editor`. One block per source line that has a `.scope()` call
//! (multiple channels stack inside the same block).
//!
//! The audio thread fills `ScopeTarget::samples` ring buffers; this renderer
//! reads them on each repaint. Repaints are driven by a timer in
//! `EditorView` that periodically calls `cx.notify()` on the editor so
//! gpui re-runs the block render closure.

use std::collections::HashMap;
use std::sync::Arc;

use editor::Editor;
use editor::display_map::{
    BlockContext, BlockPlacement, BlockProperties, BlockStyle, CustomBlockId, RenderBlock,
};
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, ParentElement, Styled, Window, div, px, rgb,
};
use language::Point;

use crate::dsl_state::ScopeTarget;

/// Insert one inline block per scope source line into the editor, replacing
/// any blocks previously inserted by `apply`. Returns the new block IDs so
/// the caller can hand them back on the next call for removal.
pub fn apply(
    editor: &Entity<Editor>,
    targets: &[ScopeTarget],
    previous: Vec<CustomBlockId>,
    cx: &mut App,
) -> Vec<CustomBlockId> {
    // Group targets by source line. One block per line, multi-channel scopes
    // stack inside the block.
    let mut by_line: HashMap<u32, Vec<ScopeTarget>> = HashMap::new();
    for t in targets {
        let Some(line) = t.source_line else { continue };
        by_line.entry(line).or_default().push(t.clone());
    }

    editor.update(cx, |editor, cx| {
        // Tear down old blocks first.
        if !previous.is_empty() {
            let set: collections::HashSet<CustomBlockId> = previous.into_iter().collect();
            editor.remove_blocks(set, None, cx);
        }

        if by_line.is_empty() {
            return Vec::new();
        }

        let buffer = editor.buffer().clone();
        let snapshot = buffer.read(cx).snapshot(cx);
        let mut props: Vec<BlockProperties<multi_buffer::Anchor>> = Vec::new();
        for (line_1based, group) in by_line {
            // DSL emits 1-based line numbers; convert to 0-based for Point.
            let row = line_1based.saturating_sub(1);
            let max_row = snapshot.max_row().0;
            let row = row.min(max_row);
            let line_len = snapshot.line_len(multi_buffer::MultiBufferRow(row));
            let point = Point::new(row, line_len);
            let anchor = snapshot.anchor_after(point);

            let group = Arc::new(group);
            let render: RenderBlock = {
                let group = group.clone();
                Arc::new(move |bcx: &mut BlockContext| -> AnyElement {
                    render_scope_block(&group, bcx)
                })
            };

            // Reserve enough vertical space for the channels — one row of
            // ~4 line-heights per channel in the group.
            let height = (group.len() as u32 * 4).max(4);
            props.push(BlockProperties {
                placement: BlockPlacement::Below(anchor),
                height: Some(height),
                style: BlockStyle::Flex,
                render,
                priority: 0,
            });
        }
        editor.insert_blocks(props, None, cx)
    })
}

fn render_scope_block(targets: &[ScopeTarget], bcx: &mut BlockContext) -> AnyElement {
    let line_height = bcx.line_height;
    let em_width = bcx.em_width;
    let mut container = div()
        .ml(bcx.anchor_x)
        .pr(px(8.))
        .py_1()
        .flex()
        .flex_col()
        .gap(px(2.));

    for target in targets {
        let waveform = render_waveform(target);
        let label = format!(
            "{}  range [{}, {}]",
            target.label, target.range.0, target.range.1
        );
        container = container.child(
            div()
                .flex()
                .flex_col()
                .h(line_height * 4)
                .child(
                    div()
                        .text_color(rgb(0x6b8aa6))
                        .text_size(em_width * 0.75)
                        .child(label),
                )
                .child(
                    div()
                        .text_color(rgb(0x88c4f8))
                        .font_family("Menlo")
                        .child(waveform),
                ),
        );
    }
    container.into_any_element()
}

/// 80-wide unicode-block waveform of the most recent samples.
fn render_waveform(target: &ScopeTarget) -> String {
    const WIDTH: usize = 120;
    let glyphs = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let ring = target.samples.lock();
    if ring.is_empty() {
        return "·".repeat(WIDTH);
    }
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
