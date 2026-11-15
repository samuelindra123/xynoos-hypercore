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
    pub physical_budget_bytes: u64,
    pub stall_budget_override: Option<u64>,
    pub stall_raw_only: bool,
    pub metadata_bytes: u64,
    pub temporary_budget_bytes: u64,
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
pub fn physical_usage(s: &SystemState) -> Result<u64, Error> {
    add(add(s.used_bytes[0], s.used_bytes[1])?, s.metadata_bytes)
}
pub fn feasible(s: &SystemState, p: &PageGroupProfile, t: Tier) -> Result<bool, Error> {
    if t == p.tier {
        return Ok(true);
    }
    let mut projected = s.clone();
    projected.used_bytes[p.tier.index()] = projected.used_bytes[p.tier.index()]
        .checked_sub(p.bytes(p.tier))
        .ok_or(Error::Capacity)?;
    projected.used_bytes[t.index()] = add(projected.used_bytes[t.index()], p.bytes(t))?;
    Ok(physical_usage(&projected)? <= s.physical_budget_bytes
        && projected.used_bytes[t.index()] <= s.capacity_bytes[t.index()]
        && p.bytes(t) <= s.temporary_budget_bytes
        && (t != Tier::Compressed || p.compress_ns <= s.cpu_budget_ns)
        && (t != Tier::Backing || p.raw_bytes <= s.io_budget_bytes))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Lru,
    CompressionFirst,
    Aura,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub fallback_recency: bool,
    pub use_cost: bool,
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
            fallback_recency: true,
            use_cost: true,
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
        if s.emergency {
            return Ok(Decision::Fallback);
        }
        if self.policy == Policy::Aura && p.confidence_permille < self.config.confidence_min {
            if !self.fallback_recency {
                return Ok(Decision::Fallback);
            }
            let mut fallback = self.clone();
            fallback.policy = Policy::Lru;
            let decision = fallback.decide_inner(s, p, tick)?;
            self.active = fallback.active;
            return Ok(decision);
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
                Policy::Aura => {
                    if self.use_cost {
                        candidate < score
                    } else {
                        p.tier == Tier::Ram
                            && ((t == Tier::Compressed && p.compressed_bytes < p.raw_bytes)
                                || (t == Tier::Backing && best == Tier::Ram))
                    }
                }
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
    pub peak_total_bytes: u64,
    pub last_feasible_mask: u8,
    pub last_capacity_rejections: u64,
    pub last_budget_rejections: u64,
    pub peak_physical_bytes: u64,
    pub peak_temporary_bytes: u64,
    pub capacity_rejections: u64,
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
        if physical_usage(&state)? > state.physical_budget_bytes {
            return Err(Error::Capacity);
        }
        let metrics = Metrics {
            peak_physical_bytes: physical_usage(&state)?,
            peak_total_bytes: physical_usage(&state)?,
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
        self.event_candidate(index, pressure, (self.tick as usize) % self.pages.len())
    }
    pub fn event_candidate(
        &mut self,
        index: usize,
        pressure: u16,
        candidate: usize,
    ) -> Result<Decision, Error> {
        let mut next = self.clone();
        let d = next.step(index, pressure, candidate)?;
        *self = next;
        Ok(d)
    }
    /// Journal dua grup; clone tetap default. Semua mutasi step terbatas pada dua indeks ini.
    pub fn event_journal(
        &mut self,
        index: usize,
        pressure: u16,
        candidate: usize,
    ) -> Result<Decision, Error> {
        if index >= self.pages.len() || candidate >= self.pages.len() {
            return Err(Error::InvalidInput);
        }
        let saved = (
            self.state.clone(),
            self.engine.clone(),
            self.metrics.clone(),
            self.tick,
            self.pages[index].clone(),
            self.pages[candidate].clone(),
        );
        let result = self.step(index, pressure, candidate);
        if result.is_err() {
            self.state = saved.0;
            self.engine = saved.1;
            self.metrics = saved.2;
            self.tick = saved.3;
            self.pages[index] = saved.4;
            self.pages[candidate] = saved.5;
        }
        result
    }
    fn step(&mut self, index: usize, pressure: u16, candidate: usize) -> Result<Decision, Error> {
        if index >= self.pages.len() || candidate >= self.pages.len() || pressure > 1000 {
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
                self.metrics.peak_total_bytes = self
                    .metrics
                    .peak_total_bytes
                    .max(add(physical_usage(&self.state)?, p.raw_bytes)?);
                self.metrics.peak_temporary_bytes =
                    self.metrics.peak_temporary_bytes.max(p.raw_bytes);
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
        self.metrics.last_feasible_mask = 0;
        self.metrics.last_capacity_rejections = 0;
        self.metrics.last_budget_rejections = 0;
        let candidate_profile = &self.pages[candidate];
        let next_tick = self.tick;
        let guards = candidate_profile.eligible
            && !self.state.emergency
            && self.engine.migrations < self.engine.config.max_migrations
            && self.engine.tick_migrations < self.engine.config.rate_limit_per_tick
            && !candidate_profile
                .last_migration_tick
                .is_some_and(|x| next_tick.saturating_sub(x) < self.engine.config.cooldown_ticks);
        if guards {
            for (bit, target) in [Tier::Ram, Tier::Compressed, Tier::Backing]
                .into_iter()
                .enumerate()
            {
                if target == candidate_profile.tier {
                    continue;
                }
                if feasible(&self.state, candidate_profile, target)? {
                    self.metrics.last_feasible_mask |= 1 << bit;
                } else {
                    let mut unbudgeted = self.state.clone();
                    unbudgeted.cpu_budget_ns = u64::MAX;
                    unbudgeted.io_budget_bytes = u64::MAX;
                    unbudgeted.temporary_budget_bytes = u64::MAX;
                    if feasible(&unbudgeted, candidate_profile, target)? {
                        self.metrics.last_budget_rejections =
                            add(self.metrics.last_budget_rejections, 1)?;
                    } else {
                        self.metrics.last_capacity_rejections =
                            add(self.metrics.last_capacity_rejections, 1)?;
                    }
                }
            }
        }
        let p = &mut self.pages[candidate];
        let decision = self.engine.decide(&self.state, p, self.tick)?;
        if decision == Decision::Fallback
            || (self.engine.policy == Policy::Aura
                && p.confidence_permille < self.engine.config.confidence_min)
        {
            self.metrics.fallbacks = add(self.metrics.fallbacks, 1)?
        }
        if let Some(t) = decision.tier() {
            if !feasible(&self.state, p, t)? {
                return Err(Error::Capacity);
            }
            self.metrics.peak_total_bytes = self
                .metrics
                .peak_total_bytes
                .max(add(physical_usage(&self.state)?, p.bytes(t))?);
            self.metrics.peak_temporary_bytes = self.metrics.peak_temporary_bytes.max(p.bytes(t));
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
            && (if self.state.stall_raw_only {
                self.state.used_bytes[0]
            } else {
                physical_usage(&self.state)?
            }) > self
                .state
                .stall_budget_override
                .unwrap_or(self.state.physical_budget_bytes)
                - self
                    .state
                    .stall_budget_override
                    .unwrap_or(self.state.physical_budget_bytes)
                    / 4
        {
            self.metrics.stall_ns =
                add(self.metrics.stall_ns, self.engine.config.stall_penalty_ns)?;
        }
        for i in 0..3 {
            self.metrics.peak_bytes[i] = self.metrics.peak_bytes[i].max(self.state.used_bytes[i]);
        }
        self.metrics.peak_physical_bytes = self
            .metrics
            .peak_physical_bytes
            .max(physical_usage(&self.state)?);
        if physical_usage(&self.state)? > self.state.physical_budget_bytes {
            return Err(Error::Capacity);
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
            physical_budget_bytes: 65536 + 16 * 80,
            stall_budget_override: None,
            stall_raw_only: false,
            metadata_bytes: 16 * 80,
            temporary_budget_bytes: 8192,
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
    fn shared_transition() {
        let mut s = fixture(Policy::CompressionFirst);
        let before = physical_usage(&s.state).unwrap();
        s.event_candidate(0, 800, 4).unwrap();
        assert!(physical_usage(&s.state).unwrap() < before);
        let mut used = [0; 3];
        for p in &s.pages {
            used[p.tier.index()] += p.bytes(p.tier);
        }
        assert_eq!(used, s.state.used_bytes);
    }
    #[test]
    fn entropy_no_free_capacity() {
        let s = fixture(Policy::Aura);
        assert!(!feasible(&s.state, &s.pages[1], Tier::Compressed).unwrap());
    }
    #[test]
    fn metadata_overcommit_overflow() {
        let s = fixture(Policy::Aura);
        let mut state = s.state.clone();
        state.physical_budget_bytes = 65536;
        assert!(matches!(
            Simulator::new(state, s.pages.clone(), s.engine.clone()),
            Err(Error::Capacity)
        ));
        let mut state = s.state.clone();
        state.metadata_bytes = u64::MAX;
        assert_eq!(physical_usage(&state), Err(Error::Overflow));
    }
    #[test]
    fn atomic_error() {
        let mut s = fixture(Policy::Aura);
        let before = s.clone();
        assert_eq!(s.event_candidate(0, 800, 99), Err(Error::InvalidInput));
        assert_eq!(s, before);
        s.tick = u64::MAX;
        let before = s.clone();
        assert_eq!(s.event_candidate(0, 800, 0), Err(Error::Overflow));
        assert_eq!(s, before);
    }
    #[test]
    fn fallback_and_budget() {
        let mut s = fixture(Policy::Aura);
        s.pages[4].confidence_permille = 0;
        for _ in 0..4 {
            s.event_candidate(0, 800, 4).unwrap();
        }
        assert_eq!(s.metrics.fallbacks, 4);
        assert_eq!(s.pages[4].tier, Tier::Backing);
        let mut t = fixture(Policy::Aura);
        t.pages[4].confidence_permille = 0;
        t.state.io_budget_bytes = 0;
        t.state.cpu_budget_ns = 0;
        for _ in 0..4 {
            t.event_candidate(0, 800, 4).unwrap();
        }
        assert_eq!(t.pages[4].tier, Tier::Ram);
    }
    #[test]
    fn bounded_sequences() {
        for policy in [Policy::Lru, Policy::CompressionFirst, Policy::Aura] {
            let mut a = fixture(policy);
            let mut b = a.clone();
            for i in 0..512 {
                let pressure = if i % 2 == 0 { 690 } else { 710 };
                assert_eq!(
                    a.event_candidate(i % 16, pressure, (i + 4) % 16),
                    b.event_candidate(i % 16, pressure, (i + 4) % 16)
                );
                assert_eq!(a, b);
                assert!(physical_usage(&a.state).unwrap() <= a.state.physical_budget_bytes);
                assert!(a.metrics.migrations <= 512);
            }
        }
    }
    #[test]
    fn temp_limit() {
        let mut s = fixture(Policy::CompressionFirst);
        s.state.temporary_budget_bytes = 0;
        let before = s.state.used_bytes;
        s.event_candidate(0, 800, 4).unwrap();
        assert_eq!(before, s.state.used_bytes);
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    #[test]
    fn differential() {
        for policy in [Policy::Aura, Policy::Lru, Policy::CompressionFirst] {
            let mut a = fixture(policy);
            let mut b = a.clone();
            for i in 0..1024 {
                let index = if i % 17 == 0 { 99 } else { i % 16 };
                let candidate = (i + 3) % 16;
                assert_eq!(
                    a.event_candidate(index, 800, candidate),
                    b.event_journal(index, 800, candidate)
                );
                assert_eq!(a, b);
            }
        }
    }
    #[test]
    fn injected_overflow() {
        let mut a = fixture(Policy::Aura);
        a.metrics.migrations = u64::MAX;
        let mut b = a.clone();
        let before = a.clone();
        assert_eq!(a.event_candidate(0, 800, 4), b.event_journal(0, 800, 4));
        assert_eq!(a, b);
        assert_eq!(a, before);
    }
    #[test]
    fn scratch_simultaneous() {
        let mut s = fixture(Policy::CompressionFirst);
        let initial = physical_usage(&s.state).unwrap();
        s.event_candidate(0, 800, 4).unwrap();
        assert_eq!(s.metrics.peak_total_bytes, initial + 512);
        assert!(
            s.metrics.peak_total_bytes
                <= s.state.physical_budget_bytes + s.state.temporary_budget_bytes
        );
    }
}
