//! The `$tr808.*` and `$tr909.*` drums run without touching the heap by
//! block and sample by sample. That both paths sound the same is checked in
//! the crate, where noise seeds can be pinned.

mod block_common;

use block_common::*;
use serde_json::json;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const VOICES: [&str; 22] = [
    "$tr808.kick",
    "$tr808.snare",
    "$tr808.lowTom",
    "$tr808.midTom",
    "$tr808.highTom",
    "$tr808.lowConga",
    "$tr808.midConga",
    "$tr808.highConga",
    "$tr808.rimShot",
    "$tr808.claves",
    "$tr808.cowbell",
    "$tr808.clap",
    "$tr808.maracas",
    "$tr808.cymbal",
    "$tr808.hiHat",
    "$tr909.kick",
    "$tr909.snare",
    "$tr909.lowTom",
    "$tr909.midTom",
    "$tr909.highTom",
    "$tr909.rimShot",
    "$tr909.clap",
];

#[test]
fn allocation_free() {
    for voice in VOICES {
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
