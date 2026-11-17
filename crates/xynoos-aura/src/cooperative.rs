//! Policy for an explicitly cooperative, private corpus. No process targeting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Keep,
    Compress,
    BackOff,
}

/// The retention signal is a private application budget, never synthetic host PSI.
/// Two observations are supplied by the owned-workload state machine.
pub fn choose(sustained_retention: bool, cpu_or_io_busy: bool, cooling: bool) -> Choice {
    if cpu_or_io_busy || cooling {
        Choice::BackOff
    } else if sustained_retention {
        Choice::Compress
    } else {
        Choice::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_choice_requires_retention_and_admits_backoff() {
        assert_eq!(choose(false, false, false), Choice::Keep);
        assert_eq!(choose(true, false, false), Choice::Compress);
        assert_eq!(choose(true, true, false), Choice::BackOff);
        assert_eq!(choose(true, false, true), Choice::BackOff);
    }
}
