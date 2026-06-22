//! Minimal recursive-descent mini-notation parser for Rust test fixtures.
//!
//! The production mini-notation parser is the TypeScript `$p()` helper in
//! `src/main/dsl/miniNotation/` (a Peggy grammar). It produces the
//! `{ ast, source, all_spans }` payload that `$cycle` / `$p.s` ship in
//! the patch graph, which Rust deserializes into a [`MiniAST`] and lowers
//! through [`super::convert`].
//!
//! This module exists so Rust-side fixtures — both in-crate `#[cfg(test)]`
//! modules and the integration tests under `crates/modular_core/tests/` —
//! can build `MiniAST` values from compact source strings instead of
//! hand-rolling deep enum trees. It implements only the subset of the TS
//! grammar those fixtures exercise (e.g. for `sp_combine` parity coverage)
//! and is not a substitute for `$p()` on the patch-graph path.
//!
//! Compiled unconditionally (not under `#[cfg(test)]`) so the integration
//! test crate — which can't see cfg-test items from this lib — can call it.
//! Module-wide `dead_code` is allowed because nothing here is reachable
//! from the production runtime.
//!
//! Grammar differences from the TS parser are possible. New mini-notation
//! features land TS-side first; mirror them here only when an existing
//! Rust fixture needs them.
//!
//! Scope:
//! - Sequences, stacks (`,`), fast subsequences `[...]`, slow subsequences `<...>`
//! - Atoms: numbers, hz (`440hz`), notes (`c4`, `d#4`, `cb3`), rest (`~`)
//! - Note letters 'a'..'g' also parse as a note without octave.
//! - Modifiers: `*`, `/`, `!`, `?`, `@`, `(k,n,rot?)`
//! - Random choice: `a|b|c`
//! - No sample-name identifiers, no module refs, no midi/volts atoms.

#![allow(dead_code)]

use super::ast::{AtomValue, Located, MiniAST, MiniASTF64, MiniASTI32, MiniASTU32};
use crate::pattern_system::SourceSpan;

pub type ParseResult<T> = Result<T, ParseError>;

#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(input: &str) -> ParseResult<MiniAST> {
    let mut p = Parser::new(input);
    p.skip_ws();
    if p.at_end() {
        return Err(ParseError("empty input".into()));
    }
    let ast = p.stack_expr()?;
    p.skip_ws();
    if !p.at_end() {
        return Err(ParseError(format!(
            "unexpected trailing input at {}: {:?}",
            p.pos,
            p.rest()
        )));
    }
    Ok(ast)
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    seed: u64,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            seed: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn rest(&self) -> &str {
        std::str::from_utf8(&self.input[self.pos..]).unwrap_or("")
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.input.get(self.pos + offset).copied()
    }

    fn consume(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn next_seed(&mut self) -> u64 {
        let s = self.seed;
        self.seed += 1;
        s
    }

    fn stack_expr(&mut self) -> ParseResult<MiniAST> {
        let head = self.choice_expr()?;
        let mut items = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_ws();
                items.push(self.choice_expr()?);
            } else {
                break;
            }
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(MiniAST::Stack(items))
        }
    }

    /// Random choice: `a b | c d` picks one whole sequence per cycle.
    /// Operands are full sequences (space-separated), so `|` binds looser
    /// than a sequence but tighter than stack (`,`). Mirrors the grammar's
    /// `ChoiceExpr` rule.
    fn choice_expr(&mut self) -> ParseResult<MiniAST> {
        let head = self.sequence_expr()?;
        let mut choices = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b'|') {
                self.pos += 1;
                self.skip_ws();
                choices.push(self.sequence_expr()?);
            } else {
                break;
            }
        }
        if choices.len() == 1 {
            Ok(choices.pop().unwrap())
        } else {
            let seed = self.next_seed();
            Ok(MiniAST::RandomChoice(choices, seed))
        }
    }

    fn sequence_expr(&mut self) -> ParseResult<MiniAST> {
        let mut elems: Vec<(MiniAST, Option<f64>)> = Vec::new();
        loop {
            self.skip_ws();
            // `|` ends a sequence so the enclosing `choice_expr` can pick it
            // up as a random-choice separator between whole sequences.
            if self.at_end()
                || matches!(
                    self.peek(),
                    Some(b']') | Some(b'>') | Some(b')') | Some(b',') | Some(b'|')
                )
            {
                break;
            }
            let (base, weight) = self.element_with_weight()?;
            elems.push((base, weight));
        }
        if elems.is_empty() {
            return Err(ParseError("empty sequence".into()));
        }
        if elems.len() == 1 && elems[0].1.is_none() {
            Ok(elems.pop().unwrap().0)
        } else {
            Ok(MiniAST::Sequence(elems))
        }
    }

    fn element_with_weight(&mut self) -> ParseResult<(MiniAST, Option<f64>)> {
        let mut ast = self.element_base()?;
        let mut weight: Option<f64> = None;
        loop {
            match self.peek() {
                Some(b'@') => {
                    self.pos += 1;
                    let n = self.maybe_number()?;
                    // Bare `@` is weight 2 (matches Tidal/krill and `_`).
                    weight = Some(n.unwrap_or(2.0));
                }
                Some(b'*') => {
                    self.pos += 1;
                    let op = self.mod_operand_f64()?;
                    ast = MiniAST::Fast(Box::new(ast), Box::new(op));
                }
                Some(b'/') => {
                    self.pos += 1;
                    let op = self.mod_operand_f64()?;
                    ast = MiniAST::Slow(Box::new(ast), Box::new(op));
                }
                Some(b'!') => {
                    // Accumulate consecutive `!`/`!n` into one Replicate:
                    // total copies = 1 + Σ(value - 1), bare `!` = 2, `!n` = n.
                    // Matches Tidal's `pRepeat = 1 + sum es` and krill. No
                    // whitespace handling — the twin only sees adjacent `!`.
                    let mut total: i64 = 1;
                    while self.peek() == Some(b'!') {
                        self.pos += 1;
                        total += self.maybe_integer()?.unwrap_or(2) - 1;
                    }
                    if total < 0 {
                        return Err(ParseError("negative replicate count".into()));
                    }
                    ast = MiniAST::Replicate(Box::new(ast), total as u32);
                }
                Some(b'?') => {
                    self.pos += 1;
                    let prob = self.maybe_number()?;
                    let seed = self.next_seed();
                    ast = MiniAST::Degrade(Box::new(ast), prob, seed);
                }
                Some(b'(') => {
                    self.pos += 1;
                    self.skip_ws();
                    let pulses = self.mod_operand_u32()?;
                    self.skip_ws();
                    if !self.consume(b',') {
                        return Err(ParseError("expected , in euclidean".into()));
                    }
                    self.skip_ws();
                    let steps = self.mod_operand_u32()?;
                    self.skip_ws();
                    let rotation = if self.consume(b',') {
                        self.skip_ws();
                        let r = self.mod_operand_i32()?;
                        Some(Box::new(r))
                    } else {
                        None
                    };
                    self.skip_ws();
                    if !self.consume(b')') {
                        return Err(ParseError("expected ) in euclidean".into()));
                    }
                    ast = MiniAST::Euclidean {
                        pattern: Box::new(ast),
                        pulses: Box::new(pulses),
                        steps: Box::new(steps),
                        rotation,
                    };
                }
                _ => break,
            }
        }
        Ok((ast, weight))
    }

    fn element_base(&mut self) -> ParseResult<MiniAST> {
        self.skip_ws();
        match self.peek() {
            Some(b'[') => self.fast_sub(),
            Some(b'<') => self.slow_sub(),
            _ => self.atom(),
        }
    }

    fn fast_sub(&mut self) -> ParseResult<MiniAST> {
        self.consume(b'[');
        self.skip_ws();
        let s = self.stack_expr()?;
        self.skip_ws();
        if !self.consume(b']') {
            return Err(ParseError("expected ]".into()));
        }
        Ok(match s {
            MiniAST::Stack(_) => MiniAST::FastCat(vec![(s, None)]),
            MiniAST::Sequence(items) => MiniAST::FastCat(items),
            other => MiniAST::FastCat(vec![(other, None)]),
        })
    }

    fn slow_sub(&mut self) -> ParseResult<MiniAST> {
        self.consume(b'<');
        self.skip_ws();
        let s = self.stack_expr()?;
        self.skip_ws();
        if !self.consume(b'>') {
            return Err(ParseError("expected >".into()));
        }
        Ok(match s {
            MiniAST::Stack(_) => MiniAST::SlowCat(vec![(s, None)]),
            MiniAST::Sequence(items) => MiniAST::SlowCat(items),
            other => MiniAST::SlowCat(vec![(other, None)]),
        })
    }

    fn atom(&mut self) -> ParseResult<MiniAST> {
        self.skip_ws();
        match self.peek() {
            Some(b'~') => {
                let start = self.pos;
                self.pos += 1;
                Ok(MiniAST::Rest(SourceSpan::new(start, self.pos)))
            }
            _ => self.value(),
        }
    }

    fn value(&mut self) -> ParseResult<MiniAST> {
        // Try note first, then hz, then number. Note letter a-g only; but a
        // and b can also start flat accidental words — disambiguate by
        // looking at following character.
        let start = self.pos;
        let c = self
            .peek()
            .ok_or_else(|| ParseError("unexpected end".into()))?;
        if c.is_ascii_alphabetic() {
            let letter = c.to_ascii_lowercase();
            if (b'a'..=b'g').contains(&letter) {
                self.pos += 1;
                // Optional accidental: '#', 's' → sharp; 'b'/'f' only if followed by digit
                let accidental = match self.peek() {
                    Some(b'#') | Some(b's') => {
                        self.pos += 1;
                        Some('#')
                    }
                    Some(b'b') | Some(b'f') => {
                        // Match the old Pest grammar's atomic note rule:
                        // treat 'b'/'f' as flat whenever it directly follows
                        // a note letter. (The previous disambiguation was
                        // to avoid confusing it with sample-name identifiers
                        // like `bd`, but bare identifiers are no longer
                        // valid atoms in the reduced grammar.)
                        self.pos += 1;
                        Some('b')
                    }
                    _ => None,
                };
                // Optional octave
                let octave = self.maybe_integer_i32()?;
                let end = self.pos;
                return Ok(MiniAST::Pure(Located::new(
                    AtomValue::Note {
                        letter: letter as char,
                        accidental,
                        octave,
                    },
                    start,
                    end,
                )));
            }
            return Err(ParseError(format!(
                "unexpected letter {:?} at {}",
                c as char, start
            )));
        }
        // Number with optional hz suffix
        let n = self.number()?;
        // hz suffix?
        if self.matches_keyword_ci("hz") {
            let end = self.pos;
            return Ok(MiniAST::Pure(Located::new(AtomValue::Hz(n), start, end)));
        }
        let end = self.pos;
        Ok(MiniAST::Pure(Located::new(
            AtomValue::Number(n),
            start,
            end,
        )))
    }

    fn matches_keyword_ci(&mut self, kw: &str) -> bool {
        let bytes = kw.as_bytes();
        let end = self.pos + bytes.len();
        if end > self.input.len() {
            return false;
        }
        for (i, b) in bytes.iter().enumerate() {
            if self.input[self.pos + i].to_ascii_lowercase() != *b {
                return false;
            }
        }
        self.pos = end;
        true
    }

    fn number(&mut self) -> ParseResult<f64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digit_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            // Optional fractional part (must have digits after .)
            let after_dot = self.pos + 1;
            if self
                .input
                .get(after_dot)
                .is_some_and(|c| c.is_ascii_digit())
            {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
        }
        if self.pos == digit_start {
            return Err(ParseError(format!(
                "expected number at {}: {:?}",
                start,
                self.rest()
            )));
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        s.parse::<f64>().map_err(|e| ParseError(e.to_string()))
    }

    fn maybe_number(&mut self) -> ParseResult<Option<f64>> {
        if matches!(self.peek(), Some(b'-') | Some(b'0'..=b'9')) {
            Ok(Some(self.number()?))
        } else {
            Ok(None)
        }
    }

    fn integer(&mut self) -> ParseResult<i64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digit_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == digit_start {
            return Err(ParseError(format!("expected integer at {}", start)));
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        s.parse::<i64>().map_err(|e| ParseError(e.to_string()))
    }

    fn maybe_integer(&mut self) -> ParseResult<Option<i64>> {
        // Only treat a leading '-' as part of an integer if followed by a digit.
        let has_sign_digits = match (self.peek(), self.peek_at(1)) {
            (Some(b'-'), Some(d)) if d.is_ascii_digit() => true,
            (Some(c), _) if c.is_ascii_digit() => true,
            _ => false,
        };
        if has_sign_digits {
            Ok(Some(self.integer()?))
        } else {
            Ok(None)
        }
    }

    fn maybe_integer_i32(&mut self) -> ParseResult<Option<i32>> {
        Ok(self.maybe_integer()?.map(|v| v as i32))
    }

    // ------ modifier operand parsers ------

    fn mod_operand_f64(&mut self) -> ParseResult<MiniASTF64> {
        match self.peek() {
            Some(b'[') => {
                self.consume(b'[');
                self.skip_ws();
                let s = self.stack_expr_f64()?;
                self.skip_ws();
                if !self.consume(b']') {
                    return Err(ParseError("expected ]".into()));
                }
                Ok(match s {
                    MiniASTF64::Stack(_) => MiniASTF64::FastCat(vec![(s, None)]),
                    MiniASTF64::Sequence(items) => MiniASTF64::FastCat(items),
                    other => MiniASTF64::FastCat(vec![(other, None)]),
                })
            }
            Some(b'<') => {
                self.consume(b'<');
                self.skip_ws();
                let s = self.stack_expr_f64()?;
                self.skip_ws();
                if !self.consume(b'>') {
                    return Err(ParseError("expected >".into()));
                }
                Ok(match s {
                    MiniASTF64::Stack(_) => MiniASTF64::SlowCat(vec![(s, None)]),
                    MiniASTF64::Sequence(items) => MiniASTF64::SlowCat(items),
                    other => MiniASTF64::SlowCat(vec![(other, None)]),
                })
            }
            _ => {
                let start = self.pos;
                let n = self.number()?;
                let end = self.pos;
                Ok(MiniASTF64::Pure(Located::new(n, start, end)))
            }
        }
    }

    fn stack_expr_f64(&mut self) -> ParseResult<MiniASTF64> {
        let head = self.sequence_expr_f64()?;
        let mut items = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_ws();
                items.push(self.sequence_expr_f64()?);
            } else {
                break;
            }
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(MiniASTF64::Stack(items))
        }
    }

    fn sequence_expr_f64(&mut self) -> ParseResult<MiniASTF64> {
        let mut elems: Vec<(MiniASTF64, Option<f64>)> = Vec::new();
        loop {
            self.skip_ws();
            if self.at_end()
                || matches!(
                    self.peek(),
                    Some(b']') | Some(b'>') | Some(b')') | Some(b',')
                )
            {
                break;
            }
            let base = self.mod_operand_f64()?;
            self.skip_ws();
            let weight = if self.peek() == Some(b'@') {
                self.pos += 1;
                self.maybe_number()?
            } else {
                None
            };
            elems.push((base, weight));
        }
        if elems.is_empty() {
            return Err(ParseError("empty f64 sequence".into()));
        }
        if elems.len() == 1 && elems[0].1.is_none() {
            Ok(elems.pop().unwrap().0)
        } else {
            Ok(MiniASTF64::Sequence(elems))
        }
    }

    fn mod_operand_u32(&mut self) -> ParseResult<MiniASTU32> {
        match self.peek() {
            Some(b'[') => {
                self.consume(b'[');
                self.skip_ws();
                let s = self.stack_expr_u32()?;
                self.skip_ws();
                if !self.consume(b']') {
                    return Err(ParseError("expected ]".into()));
                }
                Ok(match s {
                    MiniASTU32::Stack(_) => MiniASTU32::FastCat(vec![(s, None)]),
                    MiniASTU32::Sequence(items) => MiniASTU32::FastCat(items),
                    other => MiniASTU32::FastCat(vec![(other, None)]),
                })
            }
            Some(b'<') => {
                self.consume(b'<');
                self.skip_ws();
                let s = self.stack_expr_u32()?;
                self.skip_ws();
                if !self.consume(b'>') {
                    return Err(ParseError("expected >".into()));
                }
                Ok(match s {
                    MiniASTU32::Stack(_) => MiniASTU32::SlowCat(vec![(s, None)]),
                    MiniASTU32::Sequence(items) => MiniASTU32::SlowCat(items),
                    other => MiniASTU32::SlowCat(vec![(other, None)]),
                })
            }
            _ => {
                let start = self.pos;
                let n = self.integer()?;
                if n < 0 {
                    return Err(ParseError("expected non-negative integer".into()));
                }
                let end = self.pos;
                Ok(MiniASTU32::Pure(Located::new(n as u32, start, end)))
            }
        }
    }

    fn stack_expr_u32(&mut self) -> ParseResult<MiniASTU32> {
        let head = self.sequence_expr_u32()?;
        let mut items = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_ws();
                items.push(self.sequence_expr_u32()?);
            } else {
                break;
            }
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(MiniASTU32::Stack(items))
        }
    }

    fn sequence_expr_u32(&mut self) -> ParseResult<MiniASTU32> {
        let mut elems: Vec<(MiniASTU32, Option<f64>)> = Vec::new();
        loop {
            self.skip_ws();
            if self.at_end()
                || matches!(
                    self.peek(),
                    Some(b']') | Some(b'>') | Some(b')') | Some(b',')
                )
            {
                break;
            }
            let base = self.mod_operand_u32()?;
            elems.push((base, None));
        }
        if elems.is_empty() {
            return Err(ParseError("empty u32 sequence".into()));
        }
        if elems.len() == 1 && elems[0].1.is_none() {
            Ok(elems.pop().unwrap().0)
        } else {
            Ok(MiniASTU32::Sequence(elems))
        }
    }

    fn mod_operand_i32(&mut self) -> ParseResult<MiniASTI32> {
        match self.peek() {
            Some(b'[') => {
                self.consume(b'[');
                self.skip_ws();
                let s = self.stack_expr_i32()?;
                self.skip_ws();
                if !self.consume(b']') {
                    return Err(ParseError("expected ]".into()));
                }
                Ok(match s {
                    MiniASTI32::Stack(_) => MiniASTI32::FastCat(vec![(s, None)]),
                    MiniASTI32::Sequence(items) => MiniASTI32::FastCat(items),
                    other => MiniASTI32::FastCat(vec![(other, None)]),
                })
            }
            Some(b'<') => {
                self.consume(b'<');
                self.skip_ws();
                let s = self.stack_expr_i32()?;
                self.skip_ws();
                if !self.consume(b'>') {
                    return Err(ParseError("expected >".into()));
                }
                Ok(match s {
                    MiniASTI32::Stack(_) => MiniASTI32::SlowCat(vec![(s, None)]),
                    MiniASTI32::Sequence(items) => MiniASTI32::SlowCat(items),
                    other => MiniASTI32::SlowCat(vec![(other, None)]),
                })
            }
            _ => {
                let start = self.pos;
                let n = self.integer()?;
                let end = self.pos;
                Ok(MiniASTI32::Pure(Located::new(n as i32, start, end)))
            }
        }
    }

    fn stack_expr_i32(&mut self) -> ParseResult<MiniASTI32> {
        let head = self.sequence_expr_i32()?;
        let mut items = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_ws();
                items.push(self.sequence_expr_i32()?);
            } else {
                break;
            }
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(MiniASTI32::Stack(items))
        }
    }

    fn sequence_expr_i32(&mut self) -> ParseResult<MiniASTI32> {
        let mut elems: Vec<(MiniASTI32, Option<f64>)> = Vec::new();
        loop {
            self.skip_ws();
            if self.at_end()
                || matches!(
                    self.peek(),
                    Some(b']') | Some(b'>') | Some(b')') | Some(b',')
                )
            {
                break;
            }
            let base = self.mod_operand_i32()?;
            elems.push((base, None));
        }
        if elems.is_empty() {
            return Err(ParseError("empty i32 sequence".into()));
        }
        if elems.len() == 1 && elems[0].1.is_none() {
            Ok(elems.pop().unwrap().0)
        } else {
            Ok(MiniASTI32::Sequence(elems))
        }
    }
}

/// Parse a mini-notation string and convert it to a `Pattern<T>` in one
/// step. Test-only convenience for Rust fixtures: matches the call shape
/// `mini::parse(source)?` that in-crate `#[cfg(test)]` modules and the
/// `crates/modular_core/tests/` integration tests use.
pub fn parse_pattern<T: super::FromMiniAtom>(
    source: &str,
) -> Result<crate::pattern_system::Pattern<T>, super::ConvertError> {
    let ast = parse(source).map_err(|e| super::ConvertError::InvalidAtom(e.0))?;
    super::convert(&ast)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(ast: &MiniAST) -> f64 {
        match ast {
            MiniAST::Pure(located) => match located.node {
                AtomValue::Number(n) => n,
                ref other => panic!("expected number atom, got {:?}", other),
            },
            other => panic!("expected Pure atom, got {:?}", other),
        }
    }

    #[test]
    fn parses_bare_number() {
        let ast = parse("3").unwrap();
        match &ast {
            MiniAST::Pure(located) => {
                assert!(matches!(located.node, AtomValue::Number(n) if n == 3.0));
                assert_eq!(located.span.to_tuple(), (0, 1));
            }
            other => panic!("expected Pure, got {:?}", other),
        }
    }

    #[test]
    fn patterned_slow_factor_is_applied_not_collapsed() {
        use crate::pattern_system::Fraction;

        let onsets = |source: &str| -> usize {
            parse_pattern::<f64>(source)
                .unwrap()
                .query_arc(Fraction::from_integer(0), Fraction::from_integer(2))
                .iter()
                .filter(|h| h.has_onset())
                .count()
        };

        // Regression: a patterned slow factor (`/[..]`, `/<..>`) used to be
        // scalarized and fall back to slow(1), leaving the pattern un-slowed.
        // A factor of `[2 2]` is 2 everywhere, so `0/[2 2]` must slow exactly
        // like the scalar `0/2` and halve the onset density of bare `0`.
        assert_eq!(onsets("0"), 2);
        assert_eq!(onsets("0/2"), 1);
        assert_eq!(onsets("0/[2 2]"), onsets("0/2"));
        assert!(
            onsets("0/[2 2]") < onsets("0"),
            "patterned slow factor must actually slow the pattern",
        );
    }

    #[test]
    fn parses_rest() {
        let ast = parse("~").unwrap();
        match &ast {
            MiniAST::Rest(span) => assert_eq!(span.to_tuple(), (0, 1)),
            other => panic!("expected Rest, got {:?}", other),
        }
    }

    #[test]
    fn parses_sequence_of_numbers() {
        let ast = parse("0 1 2").unwrap();
        match &ast {
            MiniAST::Sequence(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(num(&items[0].0), 0.0);
                assert_eq!(num(&items[1].0), 1.0);
                assert_eq!(num(&items[2].0), 2.0);
                assert!(items.iter().all(|(_, w)| w.is_none()));
            }
            other => panic!("expected Sequence, got {:?}", other),
        }
    }

    #[test]
    fn parses_fast_subsequence_group() {
        let ast = parse("[1 2 3]").unwrap();
        match &ast {
            MiniAST::FastCat(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(num(&items[0].0), 1.0);
                assert_eq!(num(&items[1].0), 2.0);
                assert_eq!(num(&items[2].0), 3.0);
            }
            other => panic!("expected FastCat, got {:?}", other),
        }
    }

    #[test]
    fn parses_slow_subsequence_choices() {
        let ast = parse("<a b c>").unwrap();
        match &ast {
            MiniAST::SlowCat(items) => {
                assert_eq!(items.len(), 3);
                let letters: Vec<char> = items
                    .iter()
                    .map(|(child, _)| match child {
                        MiniAST::Pure(located) => match located.node {
                            AtomValue::Note { letter, .. } => letter,
                            ref other => panic!("expected Note, got {:?}", other),
                        },
                        other => panic!("expected Pure, got {:?}", other),
                    })
                    .collect();
                assert_eq!(letters, vec!['a', 'b', 'c']);
            }
            other => panic!("expected SlowCat, got {:?}", other),
        }
    }

    #[test]
    fn parses_stack_with_comma() {
        let ast = parse("0, 1").unwrap();
        match &ast {
            MiniAST::Stack(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(num(&items[0]), 0.0);
                assert_eq!(num(&items[1]), 1.0);
            }
            other => panic!("expected Stack, got {:?}", other),
        }
    }

    #[test]
    fn parses_euclidean_with_rotation() {
        let ast = parse("0(3,8,1)").unwrap();
        match &ast {
            MiniAST::Euclidean {
                pattern,
                pulses,
                steps,
                rotation,
            } => {
                assert_eq!(num(pattern), 0.0);
                match pulses.as_ref() {
                    MiniASTU32::Pure(l) => assert_eq!(l.node, 3),
                    other => panic!("expected Pure pulses, got {:?}", other),
                }
                match steps.as_ref() {
                    MiniASTU32::Pure(l) => assert_eq!(l.node, 8),
                    other => panic!("expected Pure steps, got {:?}", other),
                }
                match rotation.as_deref() {
                    Some(MiniASTI32::Pure(l)) => assert_eq!(l.node, 1),
                    other => panic!("expected Pure rotation, got {:?}", other),
                }
            }
            other => panic!("expected Euclidean, got {:?}", other),
        }
    }

    #[test]
    fn parses_elongation_weight() {
        let ast = parse("a@2 b").unwrap();
        match &ast {
            MiniAST::Sequence(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].1, Some(2.0));
                assert_eq!(items[1].1, None);
            }
            other => panic!("expected Sequence, got {:?}", other),
        }
    }

    #[test]
    fn parses_replication() {
        let ast = parse("a!3").unwrap();
        match &ast {
            MiniAST::Replicate(inner, count) => {
                assert_eq!(*count, 3);
                match inner.as_ref() {
                    MiniAST::Pure(l) => {
                        assert!(matches!(l.node, AtomValue::Note { letter: 'a', .. }))
                    }
                    other => panic!("expected Pure note, got {:?}", other),
                }
            }
            other => panic!("expected Replicate, got {:?}", other),
        }
    }

    #[test]
    fn parses_nested_groups_and_brackets() {
        // `[0 [1 2]]` — outer FastCat with a nested FastCat as the second
        // element. Confirms that bracket nesting builds nested FastCat
        // nodes rather than flattening or wrapping in Stack.
        let ast = parse("[0 [1 2]]").unwrap();
        match &ast {
            MiniAST::FastCat(outer) => {
                assert_eq!(outer.len(), 2);
                assert_eq!(num(&outer[0].0), 0.0);
                match &outer[1].0 {
                    MiniAST::FastCat(inner) => {
                        assert_eq!(inner.len(), 2);
                        assert_eq!(num(&inner[0].0), 1.0);
                        assert_eq!(num(&inner[1].0), 2.0);
                    }
                    other => panic!("expected nested FastCat, got {:?}", other),
                }
            }
            other => panic!("expected FastCat, got {:?}", other),
        }
    }

    #[test]
    fn parses_random_choice() {
        let ast = parse("0|1|2").unwrap();
        match &ast {
            MiniAST::RandomChoice(items, _seed) => {
                assert_eq!(items.len(), 3);
                assert_eq!(num(&items[0]), 0.0);
                assert_eq!(num(&items[1]), 1.0);
                assert_eq!(num(&items[2]), 2.0);
            }
            other => panic!("expected RandomChoice, got {:?}", other),
        }
    }

    #[test]
    fn parses_single_token_note_with_accidental_and_octave() {
        let ast = parse("d#4").unwrap();
        match &ast {
            MiniAST::Pure(located) => match &located.node {
                AtomValue::Note {
                    letter,
                    accidental,
                    octave,
                } => {
                    assert_eq!(*letter, 'd');
                    assert_eq!(*accidental, Some('#'));
                    assert_eq!(*octave, Some(4));
                }
                other => panic!("expected Note atom, got {:?}", other),
            },
            other => panic!("expected Pure, got {:?}", other),
        }
    }

    #[test]
    fn parses_single_token_hz_atom() {
        let ast = parse("440hz").unwrap();
        match &ast {
            MiniAST::Pure(located) => match located.node {
                AtomValue::Hz(h) => assert_eq!(h, 440.0),
                ref other => panic!("expected Hz atom, got {:?}", other),
            },
            other => panic!("expected Pure, got {:?}", other),
        }
    }

    #[test]
    fn empty_input_is_error() {
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
    }
}
