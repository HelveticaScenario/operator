//! Mini notation AST + pattern construction.
//!
//! Parsing (strings → `MiniAST`) now lives TypeScript-side in
//! `src/main/dsl/miniNotation/`. The DSL's `$p(source)` helper parses the
//! mini-notation string, then `$cycle` / `$iCycle` serialize the resulting
//! `{ ast, source, all_spans }` payload in the patch graph. Rust receives
//! that payload and lowers the AST to a `Pattern<T>` via [`convert`].
//!
//! # Feature coverage
//!
//! The grammar supports:
//! - Sequences (`0 1 2`), stacks (`a, b`), fast subsequences (`[a b]`),
//!   slow subsequences (`<a b>`)
//! - Modifiers: `*` fast, `/` slow, `!` replicate, `?` degrade,
//!   `(k,n,rot?)` euclidean, `@n` weight
//! - Random choice `a|b|c`
//! - Rests `~`
//! - Atoms: bare numbers, `Xhz` frequency, note letters with optional
//!   sharp/flat accidentals and octaves (`c4`, `d#4`, `eb4`)
//!
//! # Example (inside `modular_core`)
//! ```ignore
//! use modular_core::pattern_system::mini::{MiniAST, convert};
//! // AST built elsewhere, either parsed JS-side and deserialized, or via
//! // the `test_builders` module in tests:
//! # let ast: MiniAST = unimplemented!();
//! let pattern = convert::<f64>(&ast).unwrap();
//! ```

pub mod ast;
pub mod convert;

/// Tiny descent parser kept around only for test fixtures. Production
/// parsing is done TypeScript-side (`$p()`). See `test_parser.rs` for
/// caveats. Always compiled (not `#[cfg(test)]`) because integration
/// tests in `crates/modular_core/tests/` are a separate crate and
/// couldn't otherwise see cfg-test items from this lib.
#[doc(hidden)]
pub mod test_parser;

pub use ast::{AtomValue, Located, MiniAST, collect_leaf_spans};
pub use convert::{ConvertError, FromMiniAtom, HasRest, convert};

/// Test-only entry point: parse a mini-notation string and return the
/// resulting `Pattern<T>`. Matches the signature of the removed
/// production `mini::parse`. Exposed so in-crate test modules can keep
/// using `use crate::pattern_system::mini::parse;` unchanged.
#[doc(hidden)]
pub fn parse<T: FromMiniAtom>(
    source: &str,
) -> Result<crate::pattern_system::Pattern<T>, ConvertError> {
    test_parser::parse_pattern(source)
}

/// Test-only entry point: parse a mini-notation string into a `MiniAST`.
/// Kept so in-crate fixtures that used `mini::parse_ast` continue to work.
#[doc(hidden)]
pub fn parse_ast(
    source: &str,
) -> Result<MiniAST, test_parser::ParseError> {
    test_parser::parse(source)
}
