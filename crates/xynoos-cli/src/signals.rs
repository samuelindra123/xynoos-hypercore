//! Robust, async-signal-safe POSIX signal handling for the XYNOOS daemon.
//!
//! Replaces legacy `libc::signal` with standard POSIX `sigaction(2)`.
//! The signal handler performs ONLY an atomic boolean store (`AtomicBool::store`),
//! strictly satisfying async-signal-safety requirements (no allocations, no locks,
//! no file I/O, no complex formatting).

use std::sync::atomic::{AtomicBool, Ordering};

static DAEMON_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Returns whether a termination signal (SIGINT, SIGTERM, SIGHUP) has been delivered.
#[inline]
pub fn is_stop_requested() -> bool {
    DAEMON_STOP_REQUESTED.load(Ordering::SeqCst)
}

/// Explicitly triggers a daemon stop request (e.g. for clean testing or programmatic teardown).
#[inline]
pub fn request_stop() {
    DAEMON_STOP_REQUESTED.store(true, Ordering::SeqCst);
}

/// Resets the stop request state to false.
#[inline]
pub fn reset_stop_requested() {
    DAEMON_STOP_REQUESTED.store(false, Ordering::SeqCst);
}

#[cfg(unix)]
extern "C" fn posix_termination_handler(_signo: libc::c_int) {
    // ASYNC-SIGNAL-SAFE: only an atomic write to a static AtomicBool.
    DAEMON_STOP_REQUESTED.store(true, Ordering::SeqCst);
}

/// Installs async-signal-safe POSIX `sigaction` handlers for SIGINT, SIGTERM, and SIGHUP.
///
/// Ensures other terminating signals are masked during handler execution.
pub fn install_daemon_signals() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = posix_termination_handler as *const () as usize;
        // Block all other standard termination signals while handling
        libc::sigemptyset(&mut act.sa_mask);
        libc::sigaddset(&mut act.sa_mask, libc::SIGINT);
        libc::sigaddset(&mut act.sa_mask, libc::SIGTERM);
        libc::sigaddset(&mut act.sa_mask, libc::SIGHUP);
        act.sa_flags = 0; // Do not use SA_RESTART so blocking syscalls can return EINTR if needed

        for &sig in &[libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            if libc::sigaction(sig, &act, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_state_flags_work_atomically() {
        reset_stop_requested();
        assert!(!is_stop_requested());
        request_stop();
        assert!(is_stop_requested());
        reset_stop_requested();
        assert!(!is_stop_requested());
    }

    #[test]
    fn install_daemon_signals_succeeds() {
        assert!(install_daemon_signals().is_ok());
    }
}
