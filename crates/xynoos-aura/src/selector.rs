//! Selector deterministik dengan himpunan eligibility eksogen yang sama antar policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty,
    TooMany,
    CounterOverflow,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    cursor: usize,
    pub evaluations: u64,
    pub rejected: u64,
    pub no_candidate: u64,
}
impl Selector {
    /// Memeriksa maksimal 64 kandidat; mutasi terbatas pada state selector.
    pub fn next(&mut self, eligible: &[bool]) -> Result<Option<usize>, Error> {
        if eligible.is_empty() {
            return Err(Error::Empty);
        }
        if eligible.len() > 64 {
            return Err(Error::TooMany);
        }
        let mut next = self.clone();
        for offset in 0..eligible.len() {
            let i = (next.cursor + offset) % eligible.len();
            next.evaluations = next
                .evaluations
                .checked_add(1)
                .ok_or(Error::CounterOverflow)?;
            if eligible[i] {
                next.cursor = (i + 1) % eligible.len();
                *self = next;
                return Ok(Some(i));
            }
            next.rejected = next.rejected.checked_add(1).ok_or(Error::CounterOverflow)?;
        }
        next.no_candidate = next
            .no_candidate
            .checked_add(1)
            .ok_or(Error::CounterOverflow)?;
        next.cursor = (next.cursor + 1) % eligible.len();
        *self = next;
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_and_ties() {
        let mut s = Selector::default();
        let mask = [false, true, false, true];
        assert_eq!(s.next(&mask), Ok(Some(1)));
        assert_eq!(s.next(&mask), Ok(Some(3)));
        assert_eq!(s.next(&mask), Ok(Some(1)));
        assert_eq!(s.evaluations, 6);
        assert_eq!(s.rejected, 3);
    }
    #[test]
    fn empty_and_overflow_atomic() {
        let mut s = Selector::default();
        assert_eq!(s.next(&[]), Err(Error::Empty));
        assert_eq!(s.next(&[true; 65]), Err(Error::TooMany));
        s.evaluations = u64::MAX;
        let before = s.clone();
        assert_eq!(s.next(&[true]), Err(Error::CounterOverflow));
        assert_eq!(s, before);
    }
    #[test]
    fn no_candidate_and_changing_mask() {
        let mut s = Selector::default();
        assert_eq!(s.next(&[false; 4]), Ok(None));
        assert_eq!(s.no_candidate, 1);
        let mut seen = [false; 4];
        for _ in 0..4 {
            seen[s.next(&[true; 4]).unwrap().unwrap()] = true;
        }
        assert!(seen.into_iter().all(|x| x));
    }
}

use crate::physical::{Decision, Simulator};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub evaluations: u64,
    pub rejected: u64,
    pub feasible: u64,
    pub capacity: u64,
    pub budget: u64,
    pub no_feasible: u64,
    pub no_policy_migration: u64,
}
impl Selector {
    /// Preview memakai event clone yang sama dengan commit, termasuk layanan akses.
    /// Tidak mengubah simulator; urutan seluruh grup tetap diperiksa agar counters adil.
    pub fn state_aware(
        &mut self,
        sim: &Simulator,
        index: usize,
        pressure: u16,
        order: &[usize],
    ) -> Result<(Option<usize>, SearchStats), crate::physical::Error> {
        use crate::physical::Error;
        if order.len() != sim.pages.len() || order.is_empty() || order.len() > 64 {
            return Err(Error::InvalidInput);
        }
        let mut seen = [false; 64];
        for &i in order {
            if i >= order.len() || seen[i] {
                return Err(Error::InvalidInput);
            }
            seen[i] = true;
        }
        let mut stats = SearchStats::default();
        let mut chosen = None;
        for offset in 0..order.len() {
            let i = order[(self.cursor + offset) % order.len()];
            let mut trial = sim.clone();
            let d = trial.event_candidate(index, pressure, i)?;
            stats.evaluations += 1;
            stats.capacity += trial.metrics.last_capacity_rejections;
            stats.budget += trial.metrics.last_budget_rejections;
            if trial.metrics.last_feasible_mask != 0 {
                stats.feasible += 1
            } else {
                stats.rejected += 1;
            }
            if chosen.is_none()
                && matches!(
                    d,
                    Decision::StoreRaw | Decision::StoreCompressed | Decision::MoveToBacking
                )
            {
                chosen = Some(i);
            }
        }
        if stats.feasible == 0 {
            stats.no_feasible = 1;
        }
        if chosen.is_none() {
            stats.no_policy_migration = 1;
        }
        self.cursor = (self.cursor + 1) % order.len();
        Ok((chosen, stats))
    }
}
#[cfg(test)]
mod aware_tests {
    use super::*;
    use crate::physical::{fixture, Policy};
    #[test]
    fn checks_are_pure_and_bounded() {
        let mut sim = fixture(Policy::CompressionFirst);
        for p in &mut sim.pages {
            p.eligible = false;
        }
        let before = sim.clone();
        let mut selector = Selector::default();
        let (chosen, stats) = selector
            .state_aware(&sim, 0, 800, &(0..16).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(chosen, None);
        assert_eq!(stats.evaluations, 16);
        assert_eq!(stats.rejected, 16);
        assert_eq!(stats.no_feasible, 1);
        assert_eq!(sim, before);
    }
    #[test]
    fn multiple_rejections_progress() {
        let mut sim = fixture(Policy::CompressionFirst);
        sim.tick = 10;
        for p in &mut sim.pages {
            p.eligible = false;
        }
        sim.pages[14].eligible = true;
        let mut selector = Selector::default();
        let (chosen, stats) = selector
            .state_aware(&sim, 0, 800, &(0..16).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(chosen, Some(14));
        assert!(stats.rejected >= 15);
        let before = selector.clone();
        assert!(selector
            .state_aware(&sim, 99, 800, &(0..16).collect::<Vec<_>>())
            .is_err());
        assert_eq!(selector, before);
    }
}
