//! Primitives shared by the `$mi.*` ports, ported from `stmlib/dsp` and
//! `plaits/dsp`.

pub mod control;
pub mod filter;
#[cfg(test)]
pub mod fixture;
pub mod phase;
pub mod polyblep;
pub mod sine;
mod tables;
