//! Step functions over the arc length (speed, gradient, cant).

use serde::{Deserialize, Serialize};

/// Step profile: from `s` onwards `value` applies, until the next entry. Always sorted by `s`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepProfile<T> {
    steps: Vec<(f64, T)>,
}

impl<T: Copy> StepProfile<T> {
    pub fn constant(value: T) -> Self {
        Self {
            steps: vec![(0.0, value)],
        }
    }

    /// Builds a profile; entries are sorted. The first entry also applies for `s < s_0`.
    pub fn new(mut steps: Vec<(f64, T)>) -> Self {
        assert!(!steps.is_empty(), "StepProfile needs at least one step");
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { steps }
    }

    pub fn at(&self, s: f64) -> T {
        match self.steps.binary_search_by(|p| p.0.total_cmp(&s)) {
            Ok(i) => self.steps[i].1,
            Err(0) => self.steps[0].1,
            Err(i) => self.steps[i - 1].1,
        }
    }

    /// All steps in the range `[from, to)` — for AI and train protection look-ahead.
    pub fn steps_between(&self, from: f64, to: f64) -> impl Iterator<Item = (f64, T)> + '_ {
        self.steps
            .iter()
            .copied()
            .filter(move |(s, _)| *s >= from && *s < to)
    }

    pub fn steps(&self) -> &[(f64, T)] {
        &self.steps
    }
}

impl StepProfile<f64> {
    /// Integrates the profile from 0 to `s` (e.g. gradient ‰ → height difference in m,
    /// if the values are given in ‰ and the result is scaled by 1/1000).
    pub fn integrate(&self, s: f64) -> f64 {
        let mut acc = 0.0;
        for (i, &(s0, v)) in self.steps.iter().enumerate() {
            let start = if i == 0 { 0.0 } else { s0 };
            if start >= s {
                break;
            }
            let end = self.steps.get(i + 1).map_or(s, |n| n.0.min(s));
            acc += v * (end - start).max(0.0);
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_integrate() {
        let p = StepProfile::new(vec![(0.0, 5.0), (100.0, -10.0), (200.0, 0.0)]);
        assert_eq!(p.at(-5.0), 5.0);
        assert_eq!(p.at(0.0), 5.0);
        assert_eq!(p.at(99.9), 5.0);
        assert_eq!(p.at(100.0), -10.0);
        assert_eq!(p.at(500.0), 0.0);
        // 100 m at 5 ‰ + 50 m at -10 ‰ = 500 - 500 = 0
        assert!((p.integrate(150.0) - 0.0).abs() < 1e-9);
        assert!((p.integrate(100.0) - 500.0).abs() < 1e-9);
        assert_eq!(p.steps_between(50.0, 250.0).count(), 2);
    }
}
