//! The `$tr808.*` and `$tr909.*` drums run without touching the heap by
//! block and sample by sample. That both paths sound the same is checked in
//! the crate, where noise seeds can be pinned.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Every `$tr808.*` and `$tr909.*` voice.
fn voices() -> Vec<String> {
    modular_core::dsp::drums::schemas()
        .into_iter()
        .map(|s| s.name)
        .collect()
}

#[test]
fn allocation_free() {
    let voices = voices();
    for voice in voices.iter().map(String::as_str) {
        let trigger = if voice == "$tr808.hiHat" {
            "closed"
        } else {
            "trigger"
        };
        for mut params in [
            json!({ "trigger": poly_cable("src", CHANNELS), "accent": cable("lfo", 1) }),
            json!({ "trigger": cable("src", 0), "accent": [cable("lfo", 0), 2.0, 3.0, 4.0] }),
            json!({ "trigger": poly_cable("src", CHANNELS), "restart": true }),
        ] {
            params[trigger] = params["trigger"].take();
            if trigger != "trigger" {
                params.as_object_mut().unwrap().remove("trigger");
                params["open"] = cable("lfo", 0);
            }
            assert_eq!(allocations_while_rendering(voice, params), 0, "{voice}");
        }
    }
}
