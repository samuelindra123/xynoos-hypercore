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

// Hanya Prepared yang dapat diinspeksi; hanya CommitReady dapat dipublikasikan.
struct Prepared {
    simulation: Simulator,
    limits: (u64, u64),
}
struct CommitReady {
    simulation: Simulator,
    decision: Decision,
}
impl Prepared {
    fn new(
        original: &Simulator,
        index: usize,
        pressure: u16,
    ) -> Result<Self, crate::physical::Error> {
        let mut simulation = original.try_clone_metadata()?;
        let limits = simulation.prepare_access(index, pressure)?;
        Ok(Self { simulation, limits })
    }
    fn inspect(
        &mut self,
        index: usize,
    ) -> Result<(Decision, crate::physical::Metrics), crate::physical::Error> {
        self.simulation.inspect_prepared(index, self.limits)
    }
    fn validate(mut self, index: usize) -> Result<CommitReady, crate::physical::Error> {
        let decision = self.simulation.complete_candidate(index, self.limits)?;
        Ok(CommitReady {
            simulation: self.simulation,
            decision,
        })
    }
}
impl CommitReady {
    fn publish(self, original: &mut Simulator) -> Decision {
        *original = self.simulation;
        self.decision
    }
}

impl Selector {
    /// Satu clone transaksi dan satu layanan akses, tanpa Vec clone dalam loop kandidat.
    ///
    /// Prepared adalah detail private dan tidak dapat dipublikasikan oleh caller:
    /// ```compile_fail
    /// use xynoos_aura::selector::Prepared;
    /// ```
    pub fn fast_event(
        &mut self,
        sim: &mut Simulator,
        index: usize,
        pressure: u16,
        order: &[usize],
    ) -> Result<(Option<usize>, SearchStats, Decision), crate::physical::Error> {
        self.fast_event_inner(sim, index, pressure, order, None)
    }
    pub fn fast_event_measured(
        &mut self,
        sim: &mut Simulator,
        index: usize,
        pressure: u16,
        order: &[usize],
        inspection_ns: &mut u128,
    ) -> Result<(Option<usize>, SearchStats, Decision), crate::physical::Error> {
        self.fast_event_inner(sim, index, pressure, order, Some(inspection_ns))
    }
    fn fast_event_inner(
        &mut self,
        sim: &mut Simulator,
        index: usize,
        pressure: u16,
        order: &[usize],
        inspection_ns: Option<&mut u128>,
    ) -> Result<(Option<usize>, SearchStats, Decision), crate::physical::Error> {
        self.fast_event_hook(sim, index, pressure, order, inspection_ns, |_| Ok(()))
    }
    fn fast_event_hook<F: FnMut(u8) -> Result<(), crate::physical::Error>>(
        &mut self,
        sim: &mut Simulator,
        index: usize,
        pressure: u16,
        order: &[usize],
        inspection_ns: Option<&mut u128>,
        mut hook: F,
    ) -> Result<(Option<usize>, SearchStats, Decision), crate::physical::Error> {
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
        hook(0)?;
        let mut prepared = Prepared::new(sim, index, pressure)?;
        hook(1)?;
        let inspection_start = inspection_ns.as_ref().map(|_| std::time::Instant::now());
        let mut stats = SearchStats::default();
        let mut chosen = None;
        for offset in 0..order.len() {
            let i = order[(self.cursor + offset) % order.len()];
            let (d, metrics) = prepared.inspect(i)?;
            hook(2)?;
            stats.evaluations += 1;
            stats.capacity += metrics.last_capacity_rejections;
            stats.budget += metrics.last_budget_rejections;
            if metrics.last_feasible_mask != 0 {
                stats.feasible += 1;
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
        if let (Some(output), Some(start)) = (inspection_ns, inspection_start) {
            *output += start.elapsed().as_nanos();
        }
        if stats.feasible == 0 {
            stats.no_feasible = 1;
        }
        if chosen.is_none() {
            stats.no_policy_migration = 1;
        }
        hook(3)?;
        let ready = prepared.validate(chosen.unwrap_or(index))?;
        hook(4)?;
        let decision = ready.publish(sim);
        self.cursor = (self.cursor + 1) % order.len();
        Ok((chosen, stats, decision))
    }
}
#[cfg(test)]
mod fast_tests {
    use super::*;
    use crate::physical::{fixture, Error, Policy};
    fn reference(
        sel: &mut Selector,
        s: &mut Simulator,
        index: usize,
        p: u16,
        o: &[usize],
    ) -> Result<(Option<usize>, SearchStats, Decision), Error> {
        let mut next = sel.clone();
        let (chosen, stats) = next.state_aware(s, index, p, o)?;
        let d = s.event_candidate(index, p, chosen.unwrap_or(index))?;
        *sel = next;
        Ok((chosen, stats, d))
    }
    #[test]
    fn multi_event_equivalence() {
        for n in [1, 16, 64] {
            for policy in [Policy::Aura, Policy::Lru, Policy::CompressionFirst] {
                let mut a = fixture(policy);
                if n == 1 {
                    a.pages.truncate(1);
                    a.state.used_bytes = [4096, 0, 0];
                } else if n == 64 {
                    let template = a.pages.clone();
                    for i in 16..64 {
                        let mut p = template[i % 16].clone();
                        p.id = i as u64;
                        a.pages.push(p);
                    }
                    a.state.used_bytes = [64 * 4096, 0, 0];
                    a.state.capacity_bytes[0] = 64 * 4096;
                    a.state.physical_budget_bytes = 64 * 4096 + a.state.metadata_bytes;
                }
                let mut b = a.clone();
                let mut x = Selector::default();
                let mut y = x.clone();
                let mut rng = 42u64;
                let order: Vec<_> = (0..n).rev().collect();
                for tick in 0..256 {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let index = if tick % 31 == 0 { 99 } else { rng as usize % n };
                    let pressure = if tick % 2 == 0 { 690 } else { 800 };
                    assert_eq!(
                        reference(&mut x, &mut a, index, pressure, &order),
                        y.fast_event(&mut b, index, pressure, &order)
                    );
                    assert_eq!(a, b);
                    assert_eq!(x, y);
                }
            }
        }
    }
    #[test]
    fn six_sizes_budget_and_fallback() {
        for n in [1usize, 4, 8, 16, 32, 64] {
            for mode in 0..4 {
                let base = fixture(Policy::Aura);
                let mut pages = Vec::new();
                for i in 0..n {
                    let mut p = base.pages[i % 16].clone();
                    p.id = i as u64;
                    if mode == 2 {
                        p.confidence_permille = 0;
                    }
                    pages.push(p);
                }
                let mut state = base.state.clone();
                state.used_bytes = [n as u64 * 4096, 0, 0];
                state.capacity_bytes[0] = state.used_bytes[0];
                state.capacity_bytes[1] = n as u64 * 4096;
                state.capacity_bytes[2] = n as u64 * 8192;
                state.metadata_bytes = n as u64 * 80;
                state.physical_budget_bytes = state.used_bytes[0] + state.metadata_bytes;
                if mode == 1 {
                    state.cpu_budget_ns = 0;
                    state.io_budget_bytes = 0;
                }
                if mode == 3 {
                    state.temporary_budget_bytes = 0;
                }
                let mut a = Simulator::new(state, pages, base.engine).unwrap();
                let mut b = a.clone();
                let mut x = Selector::default();
                let mut y = x.clone();
                let order: Vec<_> = (0..n).collect();
                for i in 0..128 {
                    let idx = i % n;
                    let pressure = if i % 2 == 0 { 690 } else { 800 };
                    assert_eq!(
                        reference(&mut x, &mut a, idx, pressure, &order),
                        y.fast_event(&mut b, idx, pressure, &order)
                    );
                    assert_eq!(a, b);
                    assert_eq!(x, y);
                }
            }
        }
    }
    #[test]
    fn prepared_is_private_until_validated_publication() {
        let mut original = fixture(Policy::Aura);
        let before = original.clone();
        let mut prepared = Prepared::new(&original, 0, 800).unwrap();
        prepared.inspect(0).unwrap();
        prepared.inspect(4).unwrap();
        assert_eq!(original, before);
        let ready = prepared.validate(4).unwrap();
        assert_eq!(original, before);
        let mut expected = before.clone();
        let d = expected.event_candidate(0, 800, 4).unwrap();
        assert_eq!(ready.publish(&mut original), d);
        assert_eq!(original, expected);
    }
    #[test]
    fn failures_atomic() {
        for case in 0..5 {
            let mut a = fixture(Policy::Aura);
            match case {
                0 => a.tick = u64::MAX,
                1 => a.pages[0].accesses = u64::MAX,
                2 => a.metrics.migrations = u64::MAX,
                3 => a.engine.config.capacity_ns_per_byte = u64::MAX,
                _ => a.metrics.stall_ns = u64::MAX,
            };
            let mut b = a.clone();
            let before = a.clone();
            let mut x = Selector::default();
            let mut y = x.clone();
            let o: Vec<_> = (0..16).collect();
            let r = reference(&mut x, &mut a, 0, 800, &o);
            assert!(r.is_err());
            assert_eq!(r, y.fast_event(&mut b, 0, 800, &o));
            assert_eq!(a, b);
            assert_eq!(b, before);
            assert_eq!(x, y);
        }
    }
    #[test]
    fn invalid_order_and_empty() {
        let mut a = fixture(Policy::Aura);
        let before = a.clone();
        let mut s = Selector::default();
        assert!(s.fast_event(&mut a, 0, 800, &[0; 16]).is_err());
        assert_eq!(a, before);
    }
}

#[cfg(test)]
mod injected_tests {
    use super::*;
    use crate::physical::{fixture, Error, Policy};
    #[test]
    fn stage_errors_rollback() {
        for stage in 0..5 {
            let mut s = fixture(Policy::Aura);
            let before = s.clone();
            let mut selector = Selector::default();
            let initial = selector.clone();
            let order: Vec<_> = (0..16).collect();
            let r = selector.fast_event_hook(&mut s, 0, 800, &order, None, |phase| {
                if phase == stage {
                    Err(Error::Capacity)
                } else {
                    Ok(())
                }
            });
            assert_eq!(r, Err(Error::Capacity));
            assert_eq!(s, before);
            assert_eq!(selector, initial);
        }
    }
    #[test]
    fn unwind_before_commit_rollback() {
        for stage in 0..5 {
            let mut s = fixture(Policy::Aura);
            let before = s.clone();
            let mut selector = Selector::default();
            let initial = selector.clone();
            let order: Vec<_> = (0..16).collect();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                selector.fast_event_hook(&mut s, 0, 800, &order, None, |phase| {
                    assert_ne!(phase, stage, "injection unwind");
                    Ok(())
                })
            }));
            assert!(result.is_err());
            assert_eq!(s, before);
            assert_eq!(selector, initial);
        }
    }
    #[test]
    fn targeted_reservation_atomic() {
        let mut s = fixture(Policy::Aura);
        let before = s.clone();
        let mut selector = Selector::default();
        let initial = selector.clone();
        let order: Vec<_> = (0..16).collect();
        let r = selector.fast_event_hook(&mut s, 0, 800, &order, None, |phase| {
            if phase == 0 {
                Err(Error::AllocationReservation)
            } else {
                Ok(())
            }
        });
        assert_eq!(r, Err(Error::AllocationReservation));
        assert_eq!(s, before);
        assert_eq!(selector, initial);
    }
    #[test]
    fn controlled_allocation_rejection() {
        let mut s = fixture(Policy::Aura);
        let before = s.clone();
        let mut selector = Selector::default();
        let order: Vec<_> = (0..16).collect();
        let r = selector.fast_event_hook(&mut s, 0, 800, &order, None, |phase| {
            if phase == 0 {
                let mut v = Vec::<u8>::new();
                v.try_reserve_exact(usize::MAX)
                    .map_err(|_| Error::Capacity)?;
            }
            Ok(())
        });
        assert_eq!(r, Err(Error::Capacity));
        assert_eq!(s, before);
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    use crate::physical::{fixture, Error, Policy};
    #[test]
    fn injected_reservation() {
        let s = fixture(Policy::Aura);
        let before = s.clone();
        let r = s.try_clone_metadata_with(|_, n| {
            assert_eq!(n, 16);
            Err(Error::AllocationReservation)
        });
        assert_eq!(r, Err(Error::AllocationReservation));
        assert_eq!(s, before);
        assert_eq!(
            s.try_clone_metadata_with(|_, _| Ok(())),
            Err(Error::AllocationReservation)
        );
    }
    #[test]
    fn reserve_overflow_is_distinct() {
        let s = fixture(Policy::Aura);
        let r = s.try_clone_metadata_with(|pages, _| {
            pages
                .try_reserve_exact(usize::MAX)
                .map_err(|_| Error::AllocationReservation)
        });
        assert_eq!(r, Err(Error::AllocationReservation));
        assert_eq!(s.try_clone_metadata().unwrap(), s);
    }
    #[test]
    fn same_candidate_and_access() {
        let mut a = fixture(Policy::Aura);
        let mut b = a.clone();
        let mut x = Selector::default();
        let mut y = x.clone();
        let order: Vec<_> = (0..16).collect();
        for tick in 0..128 {
            let p = tick % 16;
            let (c, stats) = x.state_aware(&a, p, 800, &order).unwrap();
            let d = a.event_candidate(p, 800, c.unwrap_or(p)).unwrap();
            assert_eq!(y.fast_event(&mut b, p, 800, &order).unwrap(), (c, stats, d));
            assert_eq!(a, b);
            assert_eq!(x, y);
        }
    }
}
