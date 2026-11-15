//! Bounded in-memory transaction microbenchmark; excludes kernel I/O and telemetry collection.
use xynoos_linux_control::{
    transaction::{run, Backend, Identity, Outcome},
    Action,
};
struct Memory(u64);
impl Backend for Memory {
    fn ready_to_restore(&mut self) -> Result<(), String> {
        Ok(()) // This in-memory fixture never starts a worker.
    }
    fn identity(&mut self) -> Result<Identity, String> {
        Ok(Identity {
            device: 1,
            inode: 2,
            control_inode: 3,
        })
    }
    fn read_low(&mut self) -> Result<u64, String> {
        Ok(self.0)
    }
    fn write_low(&mut self, value: u64) -> Result<(), String> {
        self.0 = value;
        Ok(())
    }
}
fn main() {
    let snapshot = xynoos_pressureiq::system::collect_system();
    let mut fixture = Memory(0);
    let n = 1000;
    let start = std::time::Instant::now();
    for _ in 0..n {
        let result = run(
            &mut fixture,
            &snapshot,
            Action::MemoryLow { bytes: 4096 },
            true,
        );
        assert_eq!(result.outcome, Outcome::Restored);
        std::hint::black_box(result);
    }
    println!(
        "in_memory_transaction_mean_ns={} iterations={n}",
        start.elapsed().as_nanos() / n
    );
}
