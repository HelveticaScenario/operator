//! Half-open time intervals for representing event durations.
//!
//! A TimeSpan represents an interval [begin, end) using exact rational numbers.
//! Key operations include splitting spans at cycle boundaries and computing intersections.

use super::Fraction;

/// Half-open time interval [begin, end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeSpan {
    pub begin: Fraction,
    pub end: Fraction,
}

impl TimeSpan {
    /// Create a new timespan from begin and end fractions.
    pub fn new(begin: Fraction, end: Fraction) -> Self {
        Self { begin, end }
    }

    /// Duration of this span (end - begin).
    pub fn duration(&self) -> Fraction {
        &self.end - &self.begin
    }

    /// Split a span at cycle boundaries, calling `f` once per sub-span.
    /// Each emitted span lies within a single cycle. For [0.5, 2.3) the
    /// callback receives [0.5, 1), [1, 2), [2, 2.3).
    #[inline]
    pub fn for_each_cycle_span<F: FnMut(&TimeSpan)>(&self, mut f: F) {
        // Fast path: integer-aligned query (the DSP common case — playhead
        // queries one cycle at a time at [N, N+1)).
        if self.begin.is_integer() && self.end.is_integer() {
            let begin_int = self.begin.integer_value();
            let end_int = self.end.integer_value();
            if end_int == begin_int + 1 {
                f(self);
                return;
            }
            if end_int > begin_int {
                let mut i = begin_int;
                while i < end_int {
                    let span = TimeSpan::new(
                        super::Fraction::from_integer(i),
                        super::Fraction::from_integer(i + 1),
                    );
                    f(&span);
                    i += 1;
                }
                return;
            }
            if end_int == begin_int {
                f(self);
                return;
            }
            return;
        }

        let mut begin = self.begin.clone();

        if begin == self.end {
            let span = TimeSpan::new(begin, self.end.clone());
            f(&span);
            return;
        }
        if begin > self.end {
            return;
        }

        let end_sam = self.end.sam();

        while self.end > begin {
            if begin.sam() == end_sam {
                let span = TimeSpan::new(begin.clone(), self.end.clone());
                f(&span);
                break;
            }
            let next_begin = begin.next_sam();
            let span = TimeSpan::new(begin.clone(), next_begin.clone());
            f(&span);
            begin = next_begin;
        }
    }

    /// Intersection of two spans, returns None if disjoint.
    ///
    /// Handles zero-width (point) intersections specially.
    pub fn intersection(&self, other: &TimeSpan) -> Option<TimeSpan> {
        let intersect_begin = self.begin.max_of(&other.begin);
        let intersect_end = self.end.min_of(&other.end);

        if intersect_begin > intersect_end {
            return None;
        }

        // Handle zero-width (point) intersection
        if intersect_begin == intersect_end {
            // Don't allow point intersection at the exclusive end of either span
            if intersect_begin == self.end && self.begin < self.end {
                return None;
            }
            if intersect_begin == other.end && other.begin < other.end {
                return None;
            }
        }

        Some(TimeSpan::new(intersect_begin, intersect_end))
    }

    /// Apply a function to both begin and end times.
    pub fn with_time<F>(&self, f: F) -> TimeSpan
    where
        F: Fn(&Fraction) -> Fraction,
    {
        TimeSpan::new(f(&self.begin), f(&self.end))
    }

    // ===== f64 Fast-Path Methods for DSP =====

    /// Get begin time as f64 (for fast DSP comparisons).
    #[inline]
    pub fn begin_f64(&self) -> f64 {
        self.begin.to_f64()
    }

    /// Get end time as f64 (for fast DSP comparisons).
    #[inline]
    pub fn end_f64(&self) -> f64 {
        self.end.to_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_cycles(span: &TimeSpan) -> Vec<TimeSpan> {
        let mut out = Vec::new();
        span.for_each_cycle_span(|s| out.push(s.clone()));
        out
    }

    #[test]
    fn test_span_cycles_single_cycle() {
        let span = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        let cycles = collect_cycles(&span);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], span);
    }

    #[test]
    fn test_span_cycles_multi_cycle() {
        let span = TimeSpan::new(Fraction::new(1, 2), Fraction::new(5, 2));
        let cycles = collect_cycles(&span);

        assert_eq!(cycles.len(), 3);
        assert_eq!(
            cycles[0],
            TimeSpan::new(Fraction::new(1, 2), Fraction::from_integer(1))
        );
        assert_eq!(
            cycles[1],
            TimeSpan::new(Fraction::from_integer(1), Fraction::from_integer(2))
        );
        assert_eq!(
            cycles[2],
            TimeSpan::new(Fraction::from_integer(2), Fraction::new(5, 2))
        );
    }

    #[test]
    fn test_span_cycles_point() {
        let span = TimeSpan::new(Fraction::new(1, 2), Fraction::new(1, 2));
        let cycles = collect_cycles(&span);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], span);
    }

    #[test]
    fn test_intersection() {
        let a = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(2));
        let b = TimeSpan::new(Fraction::from_integer(1), Fraction::from_integer(3));

        let intersection = a.intersection(&b);
        assert!(intersection.is_some());
        assert_eq!(
            intersection.unwrap(),
            TimeSpan::new(Fraction::from_integer(1), Fraction::from_integer(2))
        );
    }

    #[test]
    fn test_intersection_disjoint() {
        let a = TimeSpan::new(Fraction::new(0, 1), Fraction::new(1, 2));
        let b = TimeSpan::new(Fraction::new(3, 4), Fraction::new(1, 1));

        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_intersection_touching() {
        // [0, 0.5) and [0.5, 1) should NOT intersect (half-open)
        let a = TimeSpan::new(Fraction::new(0, 1), Fraction::new(1, 2));
        let b = TimeSpan::new(Fraction::new(1, 2), Fraction::from_integer(1));

        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_duration() {
        let span = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        assert_eq!(span.duration(), Fraction::new(1, 2));
    }
}
