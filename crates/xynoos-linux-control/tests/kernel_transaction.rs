#![cfg(target_os = "linux")]
//! Physical opt-in has been removed. This external consumer checks the closed gate.
use std::path::Path;
use xynoos_linux_control::transaction::DisposableCgroup;

#[test]
fn physical_constructor_is_disabled_even_with_authorization() {
    for authorized in [false, true] {
        for path in ["/sys/fs/cgroup", "/nonexistent-xynoos-parent"] {
            let error = DisposableCgroup::create(Path::new(path), authorized)
                .err()
                .expect("physical constructor must refuse");
            assert_eq!(error, "physical cgroup mutation disabled in all builds");
        }
    }
}
