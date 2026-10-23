//! A control-derived value recomputed only when its input voltage changes, so a
//! constant control costs one comparison per sample.

/// Holds `derive(input)` for the last input it saw. Inputs compare by bit
/// pattern, so a NaN input recomputes only when its bits change and never
/// matches the empty cache.
#[derive(Clone, Copy, Default)]
pub struct Cached<T> {
    entry: Option<(u32, T)>,
}

impl<T: Copy> Cached<T> {
    #[inline]
    pub fn get(&mut self, input: f32, derive: impl FnOnce(f32) -> T) -> T {
        let bits = input.to_bits();
        match self.entry {
            Some((cached, value)) if cached == bits => value,
            _ => {
                let value = derive(input);
                self.entry = Some((bits, value));
                value
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recomputes_only_on_change() {
        let mut cache = Cached::default();
        let mut calls = 0;
        for input in [1.0, 1.0, 2.0, 2.0, f32::NAN, f32::NAN, 1.0] {
            cache.get(input, |x| {
                calls += 1;
                x
            });
        }
        assert_eq!(calls, 4);
    }

    #[test]
    fn a_nan_first_input_is_derived() {
        let mut cache = Cached::default();
        assert_eq!(cache.get(f32::NAN, |_| 7), 7);
    }
}
