//! Ableton Link integration.
//!
//! Threading model (per official Link documentation in `Link.hpp`):
//!
//! * Construction, `enable()`, `enable_start_stop_sync()`, and drop of
//!   `AblLink` are documented "Realtime-safe: no" — they must run on the
//!   main thread. We package an enabled `AblLink` together with its
//!   `HostTimeFilter` and `SessionState` into [`LinkResources`] on the main
//!   thread and hand them to the audio thread via the command queue.
//!   Resources are returned via the existing garbage queue for main-thread
//!   drop.
//!
//! * Per-buffer and per-frame access on the audio thread uses only the
//!   realtime-safe API (`capture_audio_session_state`,
//!   `commit_audio_session_state`, `clock_micros`, `phase_at_time`).
//!
//! [`LinkState`] owns the audio-thread-side runtime state. The audio thread
//! never constructs or destroys an `AblLink` itself — it only swaps live
//! resources in and out via [`LinkState::install`].

use rusty_link::{AblLink, HostTimeFilter, SessionState};

use crate::audio::TransportMeter;

/// A bundle of Ableton Link runtime objects, constructed and torn down on
/// the main thread.
pub struct LinkResources {
  pub link: AblLink,
  pub host_time_filter: HostTimeFilter,
  pub session_state: SessionState,
}

/// Per-buffer Link session-state snapshot. Captured once per audio callback
/// to avoid repeated FFI calls per frame.
pub struct LinkBufferState {
  pub host_time_micros: i64,
  pub quantum: f64,
  pub tempo: f64,
  pub micros_per_sample: f64,
}

/// The bar-phase quantum used throughout the integration (4 beats = 1 bar).
const QUANTUM: f64 = 4.0;

/// Audio-thread-side Link state. Holds the live resources (when active),
/// the per-buffer snapshot, the running sample counter for the host-time
/// filter, an in-frame index, and the pending quantized-start flag.
pub struct LinkState {
  link: Option<AblLink>,
  host_time_filter: Option<HostTimeFilter>,
  session_state: Option<SessionState>,
  /// Running sample count for HostTimeFilter.
  sample_count: u64,
  /// Per-buffer Link snapshot, refreshed at the start of every audio
  /// callback by [`LinkState::capture_buffer_state`].
  buffer: Option<LinkBufferState>,
  /// Frame index within the current buffer (used to compute per-frame host
  /// times when syncing ROOT_CLOCK).
  frame_in_buffer: usize,
  /// Pending quantized start — a Start was requested but transport is
  /// waiting for the next Link bar boundary before actually playing.
  pending_start: bool,
}

impl LinkState {
  pub fn new() -> Self {
    Self {
      link: None,
      host_time_filter: None,
      session_state: None,
      sample_count: 0,
      buffer: None,
      frame_in_buffer: 0,
      pending_start: false,
    }
  }

  /// Whether Link is currently active (resources installed).
  #[inline]
  pub fn is_active(&self) -> bool {
    self.link.is_some()
  }

  /// Install or remove Link resources. Returns the previously-held resources
  /// (if any) so the caller can ship them to the garbage queue for
  /// main-thread drop. Also clears any pending start and the per-buffer
  /// snapshot whenever the live instance changes.
  pub fn install(&mut self, new: Option<Box<LinkResources>>) -> Option<Box<LinkResources>> {
    let old = self.take();

    if let Some(res) = new {
      let LinkResources {
        link,
        host_time_filter,
        session_state,
      } = *res;
      self.link = Some(link);
      self.host_time_filter = Some(host_time_filter);
      self.session_state = Some(session_state);
      self.sample_count = 0;
    }

    old
  }

  /// Take the current resources (if any), leaving Link inactive. Used both
  /// for swap (via `install`) and explicit teardown.
  fn take(&mut self) -> Option<Box<LinkResources>> {
    self.buffer = None;
    self.pending_start = false;

    match (
      self.link.take(),
      self.host_time_filter.take(),
      self.session_state.take(),
    ) {
      (Some(link), Some(host_time_filter), Some(session_state)) => Some(Box::new(LinkResources {
        link,
        host_time_filter,
        session_state,
      })),
      _ => None,
    }
  }

  /// Force-clear the pending-start flag. Called on explicit Stop so a later
  /// peer-start cannot resurrect a cancelled patch.
  #[inline]
  pub fn clear_pending_start(&mut self) {
    self.pending_start = false;
  }

  /// Audio-callback prologue: capture the session state once per buffer and
  /// reset the in-buffer frame index. No-op when Link is inactive.
  pub fn capture_buffer_state(&mut self, sample_rate: f32) {
    self.frame_in_buffer = 0;
    self.buffer = self.capture(sample_rate);
  }

  fn capture(&mut self, sample_rate: f32) -> Option<LinkBufferState> {
    let link = self.link.as_ref()?;
    let htf = self.host_time_filter.as_mut()?;
    let ss = self.session_state.as_mut()?;

    let clock_micros = link.clock_micros();
    let host_time_micros = htf.sample_time_to_host_time(clock_micros, self.sample_count);
    link.capture_audio_session_state(ss);
    let tempo = ss.tempo();
    let micros_per_sample = 1_000_000.0 / sample_rate as f64;

    Some(LinkBufferState {
      host_time_micros,
      quantum: QUANTUM,
      tempo,
      micros_per_sample,
    })
  }

  /// Check whether a pending quantized start should be released this buffer.
  /// Returns `true` exactly once — when the Link bar phase has rolled past
  /// the boundary — and clears the flag. The caller is responsible for
  /// flipping the stopped atomic and dispatching `Clock::Start`.
  pub fn check_pending_start_release(&mut self) -> bool {
    if !self.pending_start {
      return false;
    }
    let buffer = match &self.buffer {
      Some(b) => b,
      None => return false,
    };
    let ss = match &self.session_state {
      Some(s) => s,
      None => return false,
    };
    let phase = ss.phase_at_time(buffer.host_time_micros, buffer.quantum);
    if phase < 0.5 {
      self.pending_start = false;
      true
    } else {
      false
    }
  }

  /// Compute the current frame's bar-phase and tempo and pass them to
  /// `sync` so the caller can drive ROOT_CLOCK. Always advances the
  /// in-buffer frame index when Link is active. No-op when inactive.
  pub fn sync_frame<F: FnOnce(f64, f64)>(&mut self, sync: F) {
    let buffer = match &self.buffer {
      Some(b) => b,
      None => return,
    };
    if let Some(ref ss) = self.session_state {
      let frame_offset_micros =
        (self.frame_in_buffer as f64 * buffer.micros_per_sample) as i64;
      let frame_host_time = buffer.host_time_micros + frame_offset_micros;
      let phase = ss.phase_at_time(frame_host_time, buffer.quantum);
      let bar_phase = phase / buffer.quantum;
      sync(bar_phase, buffer.tempo);
    }
    self.frame_in_buffer += 1;
  }

  /// Push an explicit tempo override to the Link session via the RT-safe
  /// capture/commit pair.
  pub fn set_tempo_now(&mut self, tempo: f64) {
    if let (Some(link), Some(ss)) = (&self.link, &mut self.session_state) {
      link.capture_audio_session_state(ss);
      let time = link.clock_micros();
      ss.set_tempo(tempo, time);
      link.commit_audio_session_state(ss);
    }
  }

  /// Arm a quantized start — request `is_playing=true` + beat=0 at the next
  /// bar boundary. Sets the pending_start flag so the audio loop can
  /// release the local stopped atomic when the boundary arrives.
  pub fn request_quantized_start(&mut self) {
    if let (Some(link), Some(ss)) = (&self.link, &mut self.session_state) {
      link.capture_audio_session_state(ss);
      let time = link.clock_micros();
      ss.set_is_playing_and_request_beat_at_time(true, time, 0.0, QUANTUM);
      link.commit_audio_session_state(ss);
      self.pending_start = true;
    }
  }

  /// Propagate a stop to the Link session.
  pub fn signal_stop(&mut self) {
    if let (Some(link), Some(ss)) = (&self.link, &mut self.session_state) {
      link.capture_audio_session_state(ss);
      let time = link.clock_micros();
      ss.set_is_playing(false, time);
      link.commit_audio_session_state(ss);
    }
  }

  /// Advance the host-time filter sample counter by the number of frames
  /// processed in this buffer.
  #[inline]
  pub fn add_samples(&mut self, n: u64) {
    self.sample_count = self.sample_count.saturating_add(n);
  }

  /// Per-buffer transport-meter writes for UI display: enabled flag,
  /// peer count, current Link tempo, free-running bar phase, and the
  /// pending-start flag. When Link is inactive only `link_pending_start`
  /// is reset (the enabled/peers state is already maintained by the
  /// main thread on disable).
  pub fn write_meter(&self, meter: &TransportMeter) {
    if let Some(ref link) = self.link {
      meter.write_link_state(true, link.num_peers() as u32);
      if let Some(ref buffer) = self.buffer {
        meter.write_bpm(buffer.tempo);
        if let Some(ref ss) = self.session_state {
          let phase = ss.phase_at_time(buffer.host_time_micros, buffer.quantum);
          let bar_phase = phase / buffer.quantum;
          meter.write_link_phase(bar_phase);
        }
      }
      meter.write_link_pending_start(self.pending_start);
    } else {
      meter.write_link_pending_start(false);
    }
  }
}
