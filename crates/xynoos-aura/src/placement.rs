//! Simulator userspace; biaya input bersifat sintetis, seluruh waktu dalam ns.
use std::fmt;
const SCALE: u64 = 1000;
pub const MAX_GROUPS: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Ram,
    Compressed,
    Backing,
}
impl Tier {
    fn index(self) -> usize {
        match self {
            Self::Ram => 0,
            Self::Compressed => 1,
            Self::Backing => 2,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Keep,
    StoreCompressed,
    StoreRaw,
    MoveToBacking,
    Defer,
    Fallback,
}
impl Decision {
    fn tier(self) -> Option<Tier> {
        match self {
            Self::StoreRaw => Some(Tier::Ram),
            Self::StoreCompressed => Some(Tier::Compressed),
            Self::MoveToBacking => Some(Tier::Backing),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidInput,
    Capacity,
    Overflow,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
fn add(a: u64, b: u64) -> Result<u64, Error> {
    a.checked_add(b).ok_or(Error::Overflow)
}
fn mul(a: u64, b: u64) -> Result<u64, Error> {
    a.checked_mul(b).ok_or(Error::Overflow)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemState {
    pub capacity_bytes: [u64; 3],
    pub used_bytes: [u64; 3],
    pub cpu_budget_ns: u64,
    pub io_budget_bytes: u64,
    pub restore_ns: [u64; 3],
    pub pressure_permille: u16,
    pub emergency: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageGroupProfile {
    pub id: u64,
    pub raw_bytes: u64,
    pub compressed_bytes: u64,
    pub accesses: u64,
    pub last_access_tick: u64,
    pub refault_permille: u16,
    pub confidence_permille: u16,
    pub compress_ns: u64,
    pub decompress_ns: u64,
    pub tier: Tier,
    pub eligible: bool,
    pub last_migration_tick: Option<u64>,
}
impl PageGroupProfile {
    fn bytes(&self, t: Tier) -> u64 {
        if t == Tier::Compressed {
            self.compressed_bytes
        } else {
            self.raw_bytes
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub enter_pressure: u16,
    pub exit_pressure: u16,
    pub confidence_min: u16,
    pub cooldown_ticks: u64,
    pub max_migrations: u64,
    pub rate_limit_per_tick: u64,
    pub capacity_ns_per_byte: u64,
    pub refault_penalty_ns: u64,
    pub stall_penalty_ns: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enter_pressure: 700,
            exit_pressure: 500,
            confidence_min: 600,
            cooldown_ticks: 4,
            max_migrations: 512,
            rate_limit_per_tick: 2,
            capacity_ns_per_byte: 4,
            refault_penalty_ns: 5000,
            stall_penalty_ns: 10000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub restore_ns: u64,
    pub cpu_ns: u64,
    pub io_ns: u64,
    pub capacity_ns: u64,
    pub refault_ns: u64,
    pub stall_ns: u64,
    pub total_ns: u64,
}
fn validate(s: &SystemState, p: &PageGroupProfile, c: Config) -> Result<(), Error> {
    if s.pressure_permille > 1000
        || p.refault_permille > 1000
        || p.confidence_permille > 1000
        || p.raw_bytes == 0
        || p.compressed_bytes == 0
        || c.exit_pressure >= c.enter_pressure
        || c.enter_pressure > 1000
        || c.confidence_min > 1000
        || s.used_bytes
            .iter()
            .zip(s.capacity_bytes)
            .any(|(u, c)| *u > c)
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}
/// Bobot kapasitas ns/byte; probabilitas dan tekanan dinormalisasi /1000.
pub fn cost(s: &SystemState, p: &PageGroupProfile, t: Tier, c: Config) -> Result<Cost, Error> {
    validate(s, p, c)?;
    let probability = u64::from(p.refault_permille);
    let restore_ns = mul(s.restore_ns[t.index()], probability)? / SCALE;
    let cpu_ns = if t == Tier::Compressed {
        add(
            if p.tier != t { p.compress_ns } else { 0 },
            mul(p.decompress_ns, probability)? / SCALE,
        )?
    } else {
        0
    };
    let io_ns = if t == Tier::Backing && p.tier != t {
        s.restore_ns[2]
    } else {
        0
    };
    let capacity_ns = if t == Tier::Backing {
        0
    } else {
        mul(
            mul(p.bytes(t), c.capacity_ns_per_byte)?,
            u64::from(s.pressure_permille),
        )? / SCALE
    };
    let refault_ns = if t != Tier::Ram {
        mul(c.refault_penalty_ns, probability)? / SCALE
    } else {
        0
    };
    let stall_ns = if t == Tier::Ram && s.pressure_permille >= c.enter_pressure {
        c.stall_penalty_ns
    } else {
        0
    };
    let total_ns = [restore_ns, cpu_ns, io_ns, capacity_ns, refault_ns, stall_ns]
        .into_iter()
        .try_fold(0, add)?;
    Ok(Cost {
        restore_ns,
        cpu_ns,
        io_ns,
        capacity_ns,
        refault_ns,
        stall_ns,
        total_ns,
    })
}
fn feasible(s: &SystemState, p: &PageGroupProfile, t: Tier) -> Result<bool, Error> {
    if t == p.tier {
        return Ok(true);
    }
    Ok(
        add(s.used_bytes[t.index()], p.bytes(t))? <= s.capacity_bytes[t.index()]
            && (t != Tier::Compressed || p.compress_ns <= s.cpu_budget_ns)
            && (t != Tier::Backing || p.raw_bytes <= s.io_budget_bytes),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Lru,
    CompressionFirst,
    Aura,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub policy: Policy,
    pub config: Config,
    pub active: bool,
    pub migrations: u64,
    pub tick_migrations: u64,
}
impl Engine {
    pub fn new(policy: Policy, config: Config) -> Self {
        Self {
            policy,
            config,
            active: false,
            migrations: 0,
            tick_migrations: 0,
        }
    }
    pub fn decide(
        &mut self,
        s: &SystemState,
        p: &PageGroupProfile,
        tick: u64,
    ) -> Result<Decision, Error> {
        let mut next = self.clone();
        let decision = next.decide_inner(s, p, tick)?;
        *self = next;
        Ok(decision)
    }
    fn decide_inner(
        &mut self,
        s: &SystemState,
        p: &PageGroupProfile,
        tick: u64,
    ) -> Result<Decision, Error> {
        validate(s, p, self.config)?;
        if p.last_access_tick > tick || p.last_migration_tick.is_some_and(|x| x > tick) {
            return Err(Error::InvalidInput);
        }
        if s.emergency
            || (self.policy == Policy::Aura && p.confidence_permille < self.config.confidence_min)
        {
            return Ok(Decision::Fallback);
        }
        if !p.eligible {
            return Ok(Decision::Keep);
        }
        if s.pressure_permille >= self.config.enter_pressure {
            self.active = true
        } else if s.pressure_permille <= self.config.exit_pressure {
            self.active = false
        }
        if !self.active {
            return Ok(Decision::Keep);
        }
        if self.migrations >= self.config.max_migrations
            || self.tick_migrations >= self.config.rate_limit_per_tick
            || p.last_migration_tick
                .is_some_and(|x| tick - x < self.config.cooldown_ticks)
        {
            return Ok(Decision::Defer);
        }
        let mut predicted = p.clone();
        if self.policy == Policy::Aura && tick - p.last_access_tick <= self.config.cooldown_ticks {
            let bonus = p.accesses.min(20) as u16 * 10;
            predicted.refault_permille = p.refault_permille.saturating_add(bonus).min(1000);
        }
        let p = &predicted;
        let mut best = p.tier;
        let mut score = cost(s, p, best, self.config)?.total_ns;
        for t in [Tier::Ram, Tier::Compressed, Tier::Backing] {
            if !feasible(s, p, t)? {
                continue;
            }
            let candidate = cost(s, p, t, self.config)?.total_ns;
            let choose = match self.policy {
                Policy::Aura => candidate < score,
                Policy::CompressionFirst => {
                    p.tier == Tier::Ram
                        && ((t == Tier::Compressed && p.compressed_bytes < p.raw_bytes)
                            || (t == Tier::Backing && best == Tier::Ram))
                }
                Policy::Lru => {
                    p.tier == Tier::Ram
                        && tick - p.last_access_tick >= self.config.cooldown_ticks
                        && t == Tier::Backing
                }
            };
            if choose {
                best = t;
                score = candidate;
            }
        }
        Ok(if best == p.tier {
            Decision::Keep
        } else {
            match best {
                Tier::Ram => Decision::StoreRaw,
                Tier::Compressed => Decision::StoreCompressed,
                Tier::Backing => Decision::MoveToBacking,
            }
        })
    }
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Metrics {
    pub refaults: u64,
    pub restore_ns: u64,
    pub stall_ns: u64,
    pub migrations: u64,
    pub compressions: u64,
    pub cpu_ns: u64,
    pub io_bytes: u64,
    pub peak_bytes: [u64; 3],
    pub fallbacks: u64,
    pub oscillations: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Simulator {
    pub state: SystemState,
    pub pages: Vec<PageGroupProfile>,
    pub engine: Engine,
    pub metrics: Metrics,
    pub tick: u64,
}
impl Simulator {
    pub fn new(
        state: SystemState,
        pages: Vec<PageGroupProfile>,
        engine: Engine,
    ) -> Result<Self, Error> {
        if pages.is_empty() || pages.len() > MAX_GROUPS {
            return Err(Error::InvalidInput);
        }
        let mut used = [0; 3];
        for (i, p) in pages.iter().enumerate() {
            validate(&state, p, engine.config)?;
            if pages[..i].iter().any(|q| q.id == p.id) {
                return Err(Error::InvalidInput);
            }
            used[p.tier.index()] = add(used[p.tier.index()], p.bytes(p.tier))?;
        }
        if used != state.used_bytes || used.iter().zip(state.capacity_bytes).any(|(u, c)| *u > c) {
            return Err(Error::Capacity);
        }
        let metrics = Metrics {
            peak_bytes: used,
            ..Metrics::default()
        };
        Ok(Self {
            state,
            pages,
            engine,
            metrics,
            tick: 0,
        })
    }
    /// Transaksi clone dibatasi MAX_GROUPS untuk rollback pada error aritmetika.
    pub fn event(&mut self, index: usize, pressure: u16) -> Result<Decision, Error> {
        let mut next = self.clone();
        let d = next.step(index, pressure)?;
        *self = next;
        Ok(d)
    }
    fn step(&mut self, index: usize, pressure: u16) -> Result<Decision, Error> {
        if index >= self.pages.len() || pressure > 1000 {
            return Err(Error::InvalidInput);
        }
        self.tick = add(self.tick, 1)?;
        self.state.pressure_permille = pressure;
        self.engine.tick_migrations = 0;
        let cpu_limit = self.state.cpu_budget_ns;
        let io_limit = self.state.io_budget_bytes;
        let p = &mut self.pages[index];
        if p.last_access_tick > self.tick || p.last_migration_tick.is_some_and(|x| x > self.tick) {
            return Err(Error::InvalidInput);
        }
        p.accesses = add(p.accesses, 1)?;
        p.last_access_tick = self.tick;
        if p.tier != Tier::Ram {
            let cpu = if p.tier == Tier::Compressed {
                p.decompress_ns
            } else {
                0
            };
            let io = if p.tier == Tier::Backing {
                p.raw_bytes
            } else {
                0
            };
            if cpu > self.state.cpu_budget_ns || io > self.state.io_budget_bytes {
                self.metrics.stall_ns =
                    add(self.metrics.stall_ns, self.engine.config.stall_penalty_ns)?;
            }
            self.state.cpu_budget_ns = self.state.cpu_budget_ns.saturating_sub(cpu);
            self.state.io_budget_bytes = self.state.io_budget_bytes.saturating_sub(io);
            self.metrics.refaults = add(self.metrics.refaults, 1)?;
            let latency = add(
                self.state.restore_ns[p.tier.index()],
                if p.tier == Tier::Compressed {
                    p.decompress_ns
                } else {
                    0
                },
            )?;
            self.metrics.restore_ns = add(self.metrics.restore_ns, latency)?;
            if p.tier == Tier::Compressed {
                self.metrics.cpu_ns = add(self.metrics.cpu_ns, p.decompress_ns)?
            } else {
                self.metrics.io_bytes = add(self.metrics.io_bytes, p.raw_bytes)?
            }
            // Service in place: tidak mempromosikan bila kapasitas/budget tidak tersedia.
            if !self.state.emergency
                && p.eligible
                && feasible(&self.state, p, Tier::Ram)?
                && self.engine.migrations < self.engine.config.max_migrations
                && self.engine.tick_migrations < self.engine.config.rate_limit_per_tick
                && !p
                    .last_migration_tick
                    .is_some_and(|x| self.tick - x < self.engine.config.cooldown_ticks)
            {
                self.state.used_bytes[p.tier.index()] = self.state.used_bytes[p.tier.index()]
                    .checked_sub(p.bytes(p.tier))
                    .ok_or(Error::Capacity)?;
                self.state.used_bytes[0] = add(self.state.used_bytes[0], p.raw_bytes)?;
                if p.last_migration_tick
                    .is_some_and(|x| self.tick - x < self.engine.config.cooldown_ticks)
                {
                    self.metrics.oscillations = add(self.metrics.oscillations, 1)?
                }
                p.tier = Tier::Ram;
                p.last_migration_tick = Some(self.tick);
                self.metrics.migrations = add(self.metrics.migrations, 1)?;
                self.engine.migrations = add(self.engine.migrations, 1)?;
                self.engine.tick_migrations = 1;
            } else {
                self.metrics.stall_ns =
                    add(self.metrics.stall_ns, self.engine.config.stall_penalty_ns)?;
            }
        }
        // Satu kandidat round-robin, tanpa scan seluruh kelompok untuk keputusan.
        let candidate = if self.engine.policy == Policy::Lru {
            self.pages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tier == Tier::Ram)
                .min_by_key(|(_, p)| (p.last_access_tick, p.id))
                .map(|(i, _)| i)
                .unwrap_or(index)
        } else {
            (self.tick as usize) % self.pages.len()
        };
        let p = &mut self.pages[candidate];
        let decision = self.engine.decide(&self.state, p, self.tick)?;
        if decision == Decision::Fallback {
            self.metrics.fallbacks = add(self.metrics.fallbacks, 1)?
        }
        if let Some(t) = decision.tier() {
            if !feasible(&self.state, p, t)? {
                return Err(Error::Capacity);
            }
            self.state.used_bytes[p.tier.index()] = self.state.used_bytes[p.tier.index()]
                .checked_sub(p.bytes(p.tier))
                .ok_or(Error::Capacity)?;
            self.state.used_bytes[t.index()] = add(self.state.used_bytes[t.index()], p.bytes(t))?;
            if t == Tier::Compressed {
                self.metrics.compressions = add(self.metrics.compressions, 1)?;
                self.metrics.cpu_ns = add(self.metrics.cpu_ns, p.compress_ns)?
            }
            if t == Tier::Backing {
                self.metrics.io_bytes = add(self.metrics.io_bytes, p.raw_bytes)?
            }
            p.tier = t;
            p.last_migration_tick = Some(self.tick);
            self.metrics.migrations = add(self.metrics.migrations, 1)?;
            self.engine.migrations = add(self.engine.migrations, 1)?;
            self.engine.tick_migrations = add(self.engine.tick_migrations, 1)?;
        }
        if pressure >= self.engine.config.enter_pressure
            && self.state.used_bytes[0]
                > self.state.capacity_bytes[0] - self.state.capacity_bytes[0] / 4
        {
            self.metrics.stall_ns =
                add(self.metrics.stall_ns, self.engine.config.stall_penalty_ns)?;
        }
        for i in 0..3 {
            self.metrics.peak_bytes[i] = self.metrics.peak_bytes[i].max(self.state.used_bytes[i]);
        }
        self.state.cpu_budget_ns = cpu_limit;
        self.state.io_budget_bytes = io_limit;
        Ok(decision)
    }
}

/// Konfigurasi eksperimen sintetis, bukan hasil kalibrasi hardware.
pub fn fixture(policy: Policy) -> Simulator {
    let pages = (0..16)
        .map(|i| PageGroupProfile {
            id: i,
            raw_bytes: 4096,
            compressed_bytes: if i % 2 == 0 { 512 } else { 4114 },
            accesses: 0,
            last_access_tick: 0,
            refault_permille: if i < 4 { 900 } else { 100 },
            confidence_permille: 900,
            compress_ns: 2000,
            decompress_ns: 3000,
            tier: Tier::Ram,
            eligible: true,
            last_migration_tick: None,
        })
        .collect();
    Simulator::new(
        SystemState {
            capacity_bytes: [65536, 16384, 131072],
            used_bytes: [65536, 0, 0],
            cpu_budget_ns: 10000,
            io_budget_bytes: 8192,
            restore_ns: [0, 1000, 100000],
            pressure_permille: 0,
            emergency: false,
        },
        pages,
        Engine::new(policy, Config::default()),
    )
    .expect("valid fixture")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cost_units_and_overflow() {
        let s = fixture(Policy::Aura);
        let p = &s.pages[0];
        let c = cost(&s.state, p, Tier::Compressed, s.engine.config).unwrap();
        assert_eq!(c.restore_ns, 900);
        assert_eq!(c.cpu_ns, 4700);
        assert_eq!(c.refault_ns, 4500);
        assert_eq!(c.total_ns, 10100);
        let mut cfg = s.engine.config;
        cfg.capacity_ns_per_byte = u64::MAX;
        let mut state = s.state.clone();
        state.pressure_permille = 1000;
        assert_eq!(cost(&state, p, Tier::Ram, cfg), Err(Error::Overflow));
    }
    #[test]
    fn determinism_and_accounting() {
        for policy in [Policy::Aura, Policy::Lru, Policy::CompressionFirst] {
            let mut a = fixture(policy);
            let mut b = a.clone();
            for i in 0..512 {
                assert_eq!(a.event(i % 16, 800), b.event(i % 16, 800));
                assert_eq!(a, b);
                let mut used = [0; 3];
                for p in &a.pages {
                    used[p.tier.index()] += p.bytes(p.tier);
                }
                assert_eq!(used, a.state.used_bytes);
                assert!(used
                    .iter()
                    .zip(a.state.capacity_bytes)
                    .all(|(u, c)| *u <= c));
                assert!(a.engine.migrations <= a.engine.config.max_migrations);
            }
        }
    }
    #[test]
    fn invalid_rollback() {
        let mut s = fixture(Policy::Aura);
        let before = s.clone();
        assert_eq!(s.event(99, 800), Err(Error::InvalidInput));
        assert_eq!(s, before);
        assert_eq!(s.event(0, 1001), Err(Error::InvalidInput));
        assert_eq!(s, before);
        s.tick = u64::MAX;
        let before = s.clone();
        assert_eq!(s.event(0, 800), Err(Error::Overflow));
        assert_eq!(s, before);
    }
    #[test]
    fn hysteresis_and_cooldown() {
        let mut s = fixture(Policy::Aura);
        let mut p = s.pages[4].clone();
        s.state.pressure_permille = 800;
        assert_eq!(
            s.engine.decide(&s.state, &p, 10).unwrap(),
            Decision::StoreCompressed
        );
        s.state.pressure_permille = 600;
        s.engine.decide(&s.state, &p, 10).unwrap();
        assert!(s.engine.active);
        s.state.pressure_permille = 499;
        assert_eq!(s.engine.decide(&s.state, &p, 10).unwrap(), Decision::Keep);
        assert!(!s.engine.active);
        s.state.pressure_permille = 800;
        p.last_migration_tick = Some(9);
        assert_eq!(s.engine.decide(&s.state, &p, 10).unwrap(), Decision::Defer);
    }
    #[test]
    fn budgets_capacity_confidence() {
        let mut s = fixture(Policy::Aura);
        s.state.pressure_permille = 800;
        let p = s.pages[4].clone();
        s.state.cpu_budget_ns = 0;
        s.state.io_budget_bytes = 0;
        assert_eq!(s.engine.decide(&s.state, &p, 10).unwrap(), Decision::Keep);
        s.pages[4].confidence_permille = 0;
        assert_eq!(
            s.engine.decide(&s.state, &s.pages[4], 10).unwrap(),
            Decision::Fallback
        );
        s.engine.migrations = s.engine.config.max_migrations;
        assert_eq!(s.engine.decide(&s.state, &p, 10).unwrap(), Decision::Defer);
        s.engine.migrations = 0;
        s.engine.tick_migrations = s.engine.config.rate_limit_per_tick;
        assert_eq!(s.engine.decide(&s.state, &p, 10).unwrap(), Decision::Defer);
        s.engine.tick_migrations = 0;
        s.state.capacity_bytes[1] = 0;
        assert!(!feasible(&s.state, &p, Tier::Compressed).unwrap());
    }
    #[test]
    fn refault_and_fallback() {
        let mut s = fixture(Policy::CompressionFirst);
        s.event(0, 800).unwrap();
        assert_eq!(s.pages[1].tier, Tier::Backing);
        s.event(1, 800).unwrap();
        assert_eq!(s.metrics.refaults, 1);
        assert_eq!(s.metrics.restore_ns, 100000);
        s.state.emergency = true;
        assert_eq!(s.event(1, 800).unwrap(), Decision::Fallback);
    }
    #[test]
    fn validation() {
        let s = fixture(Policy::Aura);
        let mut pages = s.pages.clone();
        pages[1].id = pages[0].id;
        assert!(matches!(
            Simulator::new(s.state.clone(), pages, s.engine.clone()),
            Err(Error::InvalidInput)
        ));
        let mut state = s.state.clone();
        state.used_bytes[0] = 0;
        assert!(matches!(
            Simulator::new(state, s.pages.clone(), s.engine.clone()),
            Err(Error::Capacity)
        ));
        let mut p = s.pages[0].clone();
        p.refault_permille = 1001;
        assert_eq!(
            cost(&s.state, &p, Tier::Ram, s.engine.config),
            Err(Error::InvalidInput)
        );
    }
}

#[cfg(test)]
mod protection_tests {
    use super::*;
    #[test]
    fn decision_error_is_atomic() {
        let mut s = fixture(Policy::Aura);
        s.state.pressure_permille = 1000;
        s.engine.config.capacity_ns_per_byte = u64::MAX;
        let before = s.engine.clone();
        assert_eq!(
            s.engine.decide(&s.state, &s.pages[0], 1),
            Err(Error::Overflow)
        );
        assert_eq!(s.engine, before);
    }
    #[test]
    fn adversarial_limit_and_clock() {
        let mut s = fixture(Policy::Aura);
        s.engine.config.max_migrations = 8;
        for i in 0..1024 {
            s.event(i % 16, if i % 2 == 0 { 690 } else { 710 }).unwrap();
            assert!(s.metrics.migrations <= 8);
        }
        s.pages[0].last_migration_tick = Some(u64::MAX);
        let before = s.clone();
        assert_eq!(s.event(0, 800), Err(Error::InvalidInput));
        assert_eq!(s, before);
    }
}

#[cfg(test)]
mod eligibility_tests {
    use super::*;
    #[test]
    fn ineligible_access_does_not_migrate() {
        let mut s = fixture(Policy::CompressionFirst);
        s.event(0, 800).unwrap();
        assert_eq!(s.pages[1].tier, Tier::Backing);
        s.pages[1].eligible = false;
        s.event(1, 800).unwrap();
        assert_eq!(s.pages[1].tier, Tier::Backing);
    }
}
