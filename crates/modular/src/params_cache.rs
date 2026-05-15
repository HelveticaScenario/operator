//! LRU-cached params deserialization for the main thread.
//!
//! Provides two entry points:
//! - [`deserialize_params`] — for `apply_patch` / `derive_channel_count`
//!   (optionally cache-aware, strips argument spans before lookup).

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::OnceLock;

use lru::LruCache;
use parking_lot::Mutex;

use modular_core::params::{
  CachedParams, DeserializedParams, ParamsDeserializer, extract_argument_spans,
};

// ---------------------------------------------------------------------------
// Static registries
// ---------------------------------------------------------------------------

static PARAMS_DESERIALIZERS: OnceLock<HashMap<String, ParamsDeserializer>> = OnceLock::new();

fn get_params_deserializers() -> &'static HashMap<String, ParamsDeserializer> {
  PARAMS_DESERIALIZERS.get_or_init(|| modular_core::dsp::get_params_deserializers())
}

static PARAMS_CACHE: OnceLock<Mutex<LruCache<(String, serde_json::Value), CachedParams>>> =
  OnceLock::new();

fn get_params_cache() -> &'static Mutex<LruCache<(String, serde_json::Value), CachedParams>> {
  PARAMS_CACHE.get_or_init(|| Mutex::new(LruCache::new(NonZeroUsize::new(500).unwrap())))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Deserialize params with optional LRU cache (for apply_patch / derive_channel_count).
///
/// Strips argument spans before cache lookup so identical param values at
/// different source positions share the same cache entry. Spans are extracted
/// fresh and attached to the returned `DeserializedParams`.
///
/// If `with_cache` is false, skips the cache and always deserializes fresh for set_module_param / slider path.
/// Slider interactions produce many intermediate values that would pollute
/// the cache.
pub fn deserialize_params(
  module_type: &str,
  params: serde_json::Value,
  with_cache: bool,
) -> Result<DeserializedParams, modular_core::param_errors::ModuleParamErrors> {
  let (stripped, argument_spans) = extract_argument_spans(params);
  let key = (module_type.to_string(), stripped.clone());

  // Check cache
  if with_cache {
    let mut cache = get_params_cache().lock();
    if let Some(cached) = cache.get(&key) {
      return Ok(DeserializedParams {
        params: cached.params.clone(),
        argument_spans,
        channel_count: cached.channel_count,
      });
    }
  }

  // Cache miss — deserialize
  let deserializer = get_params_deserializers().get(module_type).ok_or_else(|| {
    let mut errors = modular_core::param_errors::ModuleParamErrors::default();
    errors.add(
      String::new(),
      format!("No params deserializer for module type: {}", module_type),
    );
    errors
  })?;
  let cached = deserializer(stripped)?;

  // Store in cache
  if with_cache {
    let mut cache = get_params_cache().lock();
    cache.put(key, cached.clone());
  }

  Ok(DeserializedParams {
    params: cached.params,
    argument_spans,
    channel_count: cached.channel_count,
  })
}
