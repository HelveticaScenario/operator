//! Live section values of a `ui: true` EQ, published to the editor panel so it
//! can draw modulated sections at the values the filters are running at.

use std::any::Any;

use serde_json::{Value, json};

use super::params::MAX_EQ_SECTIONS;
use crate::module_state::{ModuleLiveState, ModuleStateMeta};
use crate::poly::PORT_MAX_CHANNELS;

/// `[freq V/Oct, gain dB, q]` of one section.
pub(super) type SectionValues = [f32; 3];

/// Room for every list the engine allows, allocated on the main thread, so
/// the slot fits any params and the audio thread only overwrites it.
pub struct EqLiveState {
    counts: Vec<u8>,
    values: Vec<[SectionValues; MAX_EQ_SECTIONS]>,
    /// Lists written since the last reset.
    used: usize,
}

impl EqLiveState {
    fn new() -> Self {
        Self {
            counts: vec![0; PORT_MAX_CHANNELS],
            values: vec![[[0.0; 3]; MAX_EQ_SECTIONS]; PORT_MAX_CHANNELS],
            used: 0,
        }
    }

    /// Record list `l`'s section values; extra lists or sections are dropped.
    pub(super) fn write_list(&mut self, l: usize, sections: impl Iterator<Item = SectionValues>) {
        let (Some(count), Some(slot)) = (self.counts.get_mut(l), self.values.get_mut(l)) else {
            return;
        };
        let mut n = 0;
        for (dst, v) in slot.iter_mut().zip(sections) {
            *dst = v;
            n += 1;
        }
        *count = n as u8;
        self.used = self.used.max(l + 1);
    }
}

impl ModuleLiveState for EqLiveState {
    fn reset(&mut self) {
        self.counts.fill(0);
        self.used = 0;
    }

    /// Polls snapshot the slot under the lock the audio thread also takes, so
    /// only the written lists are copied.
    fn clone_box(&self) -> Box<dyn ModuleLiveState> {
        Box::new(Self {
            counts: self.counts[..self.used].to_vec(),
            values: self.values[..self.used].to_vec(),
            used: self.used,
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct EqLiveMeta;

impl ModuleStateMeta for EqLiveMeta {
    /// Shape: `{ "eqLive": [[[freq, gain, q], ...per section], ...per list] }`,
    /// freq in V/Oct.
    fn build_json(&self, live: &dyn ModuleLiveState) -> Value {
        let Some(live) = live.as_any().downcast_ref::<EqLiveState>() else {
            return Value::Null;
        };
        let lists: Vec<Value> = live.counts[..live.used]
            .iter()
            .zip(&live.values)
            .map(|(&n, values)| json!(&values[..n as usize]))
            .collect();
        json!({ "eqLive": lists })
    }
}

/// Only EQs showing a panel publish live values.
pub fn eq_state_builder(
    params: &Value,
) -> Option<(Box<dyn ModuleLiveState>, Box<dyn ModuleStateMeta>)> {
    if params.get("ui") != Some(&Value::Bool(true)) {
        return None;
    }
    Some((Box::new(EqLiveState::new()), Box::new(EqLiveMeta)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_skips_non_ui_eqs() {
        assert!(eq_state_builder(&json!({ "sections": [{ "freq": 0 }] })).is_none());
        assert!(eq_state_builder(&json!({ "ui": true })).is_some());
    }

    #[test]
    fn json_carries_only_written_lists_and_sections() {
        let mut live = EqLiveState::new();
        live.write_list(0, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]].into_iter());
        live.write_list(1, std::iter::once([7.0, 8.0, 9.0]));
        live.write_list(PORT_MAX_CHANNELS, std::iter::once([0.0; 3]));
        assert_eq!(
            EqLiveMeta.build_json(&live),
            json!({ "eqLive": [[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]], [[7.0, 8.0, 9.0]]] })
        );
        let snapshot = live.clone_box();
        let copied = snapshot.as_any().downcast_ref::<EqLiveState>().unwrap();
        assert_eq!(
            copied.values.len(),
            2,
            "a snapshot copies only written lists"
        );
        assert_eq!(
            EqLiveMeta.build_json(&*snapshot),
            EqLiveMeta.build_json(&live)
        );
        live.reset();
        assert_eq!(EqLiveMeta.build_json(&live), json!({ "eqLive": [] }));
    }
}
