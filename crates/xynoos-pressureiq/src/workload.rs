//! Read-only Linux process observations. A PID and its kernel start time form the identity.
//! Process counters exclude children; PSI is not exposed per process by Linux.
use crate::{read, Error};
use std::{path::PathBuf, time::Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Debug, Clone)]
pub struct Reading<T> {
    pub at: Instant,
    pub result: Result<T, Error>,
}
#[derive(Debug, Clone)]
pub struct Cpu {
    pub user_ticks: u64,
    pub system_ticks: u64,
}
#[derive(Debug, Clone)]
pub struct Io {
    pub read_bytes: u64,
    pub write_bytes: u64,
}
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub identity: Identity,
    pub cpu: Reading<Cpu>,
    pub memory_bytes: Reading<u64>,
    pub io: Reading<Io>,
    /// Linux has no per-process PSI; cgroup PSI must not be attributed to this PID.
    pub pressure: Reading<()>,
}
fn stat(pid: u32) -> Result<(Identity, Cpu), Error> {
    let text = read(&PathBuf::from(format!("/proc/{pid}/stat")))?;
    let (_, tail) = text
        .rsplit_once(") ")
        .ok_or_else(|| Error::Malformed("stat comm".into()))?;
    let f: Vec<_> = tail.split_whitespace().collect();
    let get = |i: usize| -> Result<u64, Error> {
        f.get(i)
            .ok_or_else(|| Error::Missing("stat field".into()))?
            .parse()
            .map_err(|_| Error::Malformed("stat field".into()))
    };
    Ok((
        Identity {
            pid,
            start_ticks: get(19)?,
        },
        Cpu {
            user_ticks: get(11)?,
            system_ticks: get(12)?,
        },
    ))
}
fn keyed(text: &str, key: &str) -> Result<u64, Error> {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .ok_or_else(|| Error::Missing(key.into()))?;
    line.split_whitespace()
        .next()
        .ok_or_else(|| Error::Missing(key.into()))?
        .parse()
        .map_err(|_| Error::Malformed(key.into()))
}
fn guarded<T>(id: &Identity, f: impl FnOnce() -> Result<T, Error>) -> Reading<T> {
    let at = Instant::now();
    let result = (|| {
        if stat(id.pid)?.0 != *id {
            return Err(Error::Malformed("process identity changed".into()));
        }
        let value = f()?;
        if stat(id.pid)?.0 != *id {
            return Err(Error::Malformed("process identity changed".into()));
        }
        Ok(value)
    })();
    Reading { at, result }
}
pub fn identify(pid: u32) -> Result<Identity, Error> {
    if pid == 0 {
        return Err(Error::Bounds);
    }
    Ok(stat(pid)?.0)
}
pub fn collect(id: &Identity) -> ProcessSnapshot {
    let cpu = guarded(id, || Ok(stat(id.pid)?.1));
    let memory_bytes = guarded(id, || {
        let s = read(&PathBuf::from(format!("/proc/{}/status", id.pid)))?;
        keyed(&s, "VmRSS:")?
            .checked_mul(1024)
            .ok_or_else(|| Error::Overflow("VmRSS".into()))
    });
    let io = guarded(id, || {
        let s = read(&PathBuf::from(format!("/proc/{}/io", id.pid)))?;
        Ok(Io {
            read_bytes: keyed(&s, "read_bytes:")?,
            write_bytes: keyed(&s, "write_bytes:")?,
        })
    });
    ProcessSnapshot {
        identity: id.clone(),
        cpu,
        memory_bytes,
        io,
        pressure: Reading {
            at: Instant::now(),
            result: Err(Error::Unsupported),
        },
    }
}
#[derive(Debug)]
pub struct Delta {
    pub cpu_interval: std::time::Duration,
    pub cpu_ticks: u64,
    pub io_interval: std::time::Duration,
    pub read_bytes: u64,
    pub write_bytes: u64,
}
pub fn delta(a: &ProcessSnapshot, b: &ProcessSnapshot) -> Result<Delta, Error> {
    if a.identity != b.identity {
        return Err(Error::Malformed("process identity mismatch".into()));
    }
    let interval = |x: Instant, y: Instant| {
        y.checked_duration_since(x)
            .filter(|d| !d.is_zero())
            .ok_or_else(|| Error::Malformed("monotonic interval".into()))
    };
    let (ac, bc) = (
        a.cpu.result.as_ref().map_err(Clone::clone)?,
        b.cpu.result.as_ref().map_err(Clone::clone)?,
    );
    let (ai, bi) = (
        a.io.result.as_ref().map_err(Clone::clone)?,
        b.io.result.as_ref().map_err(Clone::clone)?,
    );
    let sub = |x: u64, y: u64| {
        y.checked_sub(x)
            .ok_or_else(|| Error::CounterReset("process counter".into()))
    };
    Ok(Delta {
        cpu_interval: interval(a.cpu.at, b.cpu.at)?,
        cpu_ticks: sub(ac.user_ticks, bc.user_ticks)?
            .checked_add(sub(ac.system_ticks, bc.system_ticks)?)
            .ok_or_else(|| Error::Overflow("cpu ticks".into()))?,
        io_interval: interval(a.io.at, b.io.at)?,
        read_bytes: sub(ai.read_bytes, bi.read_bytes)?,
        write_bytes: sub(ai.write_bytes, bi.write_bytes)?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_and_intervals() {
        let id = identify(std::process::id()).unwrap();
        let a = collect(&id);
        assert!(a.cpu.result.is_ok());
        assert!(a.pressure.result.is_err());
        assert!(delta(&a, &a).is_err());
        let mut wrong = id.clone();
        wrong.start_ticks += 1;
        assert!(collect(&wrong).cpu.result.is_err());
        let mut b = a.clone();
        b.cpu.at += std::time::Duration::from_secs(1);
        b.io.at += std::time::Duration::from_secs(1);
        b.cpu.result.as_mut().unwrap().user_ticks = 0;
        let mut previous = a.clone();
        previous.cpu.result.as_mut().unwrap().user_ticks = 1;
        assert!(matches!(delta(&previous, &b), Err(Error::CounterReset(_))));
        b.io.result = Err(Error::Permission);
        assert!(matches!(delta(&a, &b), Err(Error::Permission)));
    }
}
