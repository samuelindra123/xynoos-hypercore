//! Example demonstrating opt-in application buffer management with `HyperMemoryPool`.
//!
//! Demonstrates:
//! 1. Creating a bounded pool with explicit capacity and budget.
//! 2. Storing application buffers with adaptive compression.
//! 3. Zero-copy decompression into caller-managed slices via `get_into`.
//! 4. Inspecting detailed pool accounting and net memory savings.
//! 5. Stale handle rejection and graceful cleanup.

use xynoos_core::CodecId;
use xynoos_hypermemory::{Error, HyperMemoryPool};

fn main() -> Result<(), Error> {
    println!("=== XYNOOS HyperMemoryPool Integration Example ===");

    // 1. Initialize a 4 KiB-paged pool bounded to at most 64 pages (256 KiB) and 128 KiB memory budget
    let mut pool = HyperMemoryPool::with_budget(64, 128 * 1024)?;
    println!("Pool initialized: page_size = {} bytes", pool.page_size());

    // 2. Insert representative application payloads
    let payload1 = b"Foreground application session data: ".repeat(200); // ~7.4 KiB
    let h1 = pool.put_with_codec(&payload1, CodecId::Lz4Standard)?;
    println!(
        "Stored payload 1: handle id = {}, generation = {}",
        h1.id(),
        h1.generation()
    );

    let payload2 = vec![0x42u8; 32 * 1024]; // 32 KiB of repetitive data
    let h2 = pool.put_with_codec(&payload2, CodecId::Lz4Hc { level: 6 })?;
    println!(
        "Stored payload 2: handle id = {}, generation = {}",
        h2.id(),
        h2.generation()
    );

    // 3. Inspect storage metrics and net savings
    let acct = pool.pool_accounting();
    println!("\nStorage Accounting:");
    println!("  Buffer count:           {}", acct.buffer_count);
    println!("  Logical payload bytes:  {} B", acct.logical_payload_bytes);
    println!("  Stored payload bytes:   {} B", acct.stored_payload_bytes);
    println!("  Metadata overhead:      {} B", acct.metadata_bytes);
    println!("  Net memory savings:     {} B", acct.net_savings_bytes);
    println!(
        "  Compression ratio:      {:.2}% of original size",
        acct.compression_ratio * 100.0
    );

    // 4. Retrieve data with zero heap allocation using `get_into`
    let info = pool.buffer_info(h1)?;
    println!(
        "\nRetrieving buffer 1 (length {} bytes)...",
        info.logical_len
    );
    let mut read_buf = vec![0u8; info.logical_len];
    let bytes_read = pool.get_into(h1, &mut read_buf)?;
    assert_eq!(bytes_read, payload1.len());
    assert_eq!(&read_buf, &payload1);
    println!("  Verified byte-for-byte integrity!");

    // 5. Demonstrate stale handle protection
    pool.remove(h1)?;
    println!("\nBuffer 1 removed.");
    match pool.get(h1) {
        Err(Error::NotFound) => println!("  Access with old handle correctly rejected (NotFound)!"),
        other => panic!("expected NotFound, got {other:?}"),
    }

    // 6. Demonstrate capacity enforcement
    println!("\nCapacity enforcement:");
    let oversized = vec![0x99u8; 200 * 1024]; // 200 KiB exceeds budget
    match pool.put(&oversized) {
        Err(e) => println!("  Oversized insertion correctly declined: {e}"),
        Ok(_) => panic!("expected capacity rejection"),
    }

    println!("\nHyperMemoryPool lifecycle completed successfully.");
    Ok(())
}
