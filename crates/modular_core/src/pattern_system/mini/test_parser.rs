//! Minimal mini-notation parser — not for production use.
//!
//! The production parser now lives TypeScript-side (`$p()` in
//! `src/main/dsl/miniNotation/`). Rust no longer parses mini-notation
//! strings on the production patch-graph path. But many existing
//! fixtures — both `#[cfg(test)]` unit tests and the integration tests
//! in `crates/modular_core/tests/` — were written against
//! `mini::parse(...)`, and rewriting hundreds of tests to build
//! `MiniAST` by hand would be noisy. This module preserves a thin
//! descent parser that covers only the subset of mini-notation
//! exercised by those tests.
//!
//! This is left compiled in all builds (not `#[cfg(test)]`) because
//! integration tests are a separate crate and can't see cfg-test items
//! from this lib. Unused in production: dead-code warnings are
//! suppressed module-wide.
//!
//! **Not suitable for production use.** Grammar differences from the TS
//! parser are possible; if you add a new feature, test it TS-side first,
//! then (optionally) mirror it here for existing Rust test fixtures.
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
        let head = self.sequence_expr()?;
        let mut items = vec![head];
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                self.skip_ws();
                items.push(self.sequence_expr()?);
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

    fn sequence_expr(&mut self) -> ParseResult<MiniAST> {
        let mut elems: Vec<(MiniAST, Option<f64>)> = Vec::new();
        loop {
            self.skip_ws();
            if self.at_end() || matches!(self.peek(), Some(b']') | Some(b'>') | Some(b')') | Some(b',')) {
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
                    weight = Some(n.unwrap_or(1.0));
                }
                Some(b'*') => {
                    self.pos += 1;
                    let op = self.mod_operand_f64()?;
                    ast = MiniAST::Fast(Box::new(ast), Box::new(op));
                }
                Some(b'/') => {
                    self.pos += 1;
                    let op = self.mod_operand()?;
                    ast = MiniAST::Slow(Box::new(ast), Box::new(op));
                }
                Some(b'!') => {
                    self.pos += 1;
                    let count = self.maybe_integer()?.unwrap_or(2);
                    if count < 0 {
                        return Err(ParseError("negative replicate count".into()));
                    }
                    ast = MiniAST::Replicate(Box::new(ast), count as u32);
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
            _ => self.atom_or_choice(),
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

    fn atom_or_choice(&mut self) -> ParseResult<MiniAST> {
        let first = self.choice_element()?;
        let mut choices = vec![first];
        loop {
            self.skip_ws();
            if self.peek() == Some(b'|') {
                self.pos += 1;
                self.skip_ws();
                choices.push(self.choice_element()?);
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

    fn choice_element(&mut self) -> ParseResult<MiniAST> {
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
            return Ok(MiniAST::Pure(Located::new(
                AtomValue::Hz(n),
                start,
                end,
            )));
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
            if self.input.get(after_dot).is_some_and(|c| c.is_ascii_digit()) {
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

    fn mod_operand(&mut self) -> ParseResult<MiniAST> {
        match self.peek() {
            Some(b'[') | Some(b'<') => {
                // Full stack expr — reuse element_base.
                self.element_base()
            }
            _ => {
                let start = self.pos;
                let n = self.number()?;
                let end = self.pos;
                Ok(MiniAST::Pure(Located::new(
                    AtomValue::Number(n),
                    start,
                    end,
                )))
            }
        }
    }

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
            if self.at_end() || matches!(self.peek(), Some(b']') | Some(b'>') | Some(b')') | Some(b',')) {
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
            if self.at_end() || matches!(self.peek(), Some(b']') | Some(b'>') | Some(b')') | Some(b',')) {
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
            if self.at_end() || matches!(self.peek(), Some(b']') | Some(b'>') | Some(b')') | Some(b',')) {
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
/// step. Equivalent to the old `mini::parse` production entry point, now
/// test-only. Used exclusively from `#[cfg(test)]` fixtures.
pub fn parse_pattern<T: super::FromMiniAtom>(
    source: &str,
) -> Result<crate::pattern_system::Pattern<T>, super::ConvertError> {
    let ast = parse(source).map_err(|e| super::ConvertError::InvalidAtom(e.0))?;
    super::convert(&ast)
}
