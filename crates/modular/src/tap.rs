//! Scope and VU meter reads for the audio callback's drain.

use modular_core::types::{PortView, Sampleable, SampleableMap};

/// One channel of a module output port, resolved once per drained block so the
/// per-sample reads skip the id and port-name lookups.
pub(crate) enum Tap<'a> {
    /// A Block-mode port read straight from its output buffer.
    View {
        data: *const f32,
        stride: usize,
        offset: usize,
    },
    /// A port with no raw view (Sample-mode wrappers), read through the module.
    Module {
        module: &'a dyn Sampleable,
        port: &'a str,
        channel: usize,
    },
}

impl<'a> Tap<'a> {
    /// `None` when the module is not in the patch.
    ///
    /// The tap is only valid for reads below the slot every module has been
    /// processed to; the drain resolves taps after `process_all_modules_to`
    /// has advanced the whole patch through the block.
    pub(crate) fn resolve(
        sampleables: &'a SampleableMap,
        module_id: &str,
        port: &'a str,
        channel: usize,
    ) -> Option<Self> {
        let module = sampleables.get(module_id)?.as_ref();
        Some(match module.port_view(port) {
            Some(PortView { data, channels, .. }) => Tap::View {
                data,
                stride: channels,
                offset: channel % channels,
            },
            None => Tap::Module {
                module,
                port,
                channel,
            },
        })
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> f32 {
        match *self {
            // SAFETY: `data` points into the producer's block buffer, which
            // outlives the borrowed patch, and `index` is below the processed
            // slot (see `resolve`), so the offset stays inside the buffer.
            Tap::View {
                data,
                stride,
                offset,
            } => unsafe { *data.add(index * stride + offset) },
            Tap::Module {
                module,
                port,
                channel,
            } => module.get_value_at(port, channel, index),
        }
    }
}
