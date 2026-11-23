#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use xynoos_aura::{AdaptiveTieringPolicy, AuraTieringCoordinator, PressureBridge};
use xynoos_core::{MemoryPressure, StorageTier};
use xynoos_hypermemory::PageStore;
use xynoos_pressureiq::intelligence::Diagnosis;

fn make_diagnosis(classes: Vec<&str>, avail: Option<f64>, reliable: bool) -> Diagnosis {
    Diagnosis {
        classes: classes.into_iter().map(String::from).collect(),
        evidence: vec![],
        limitations: vec![],
        available_pct: avail,
        rates_per_second: BTreeMap::new(),
        disk_bytes_per_second: BTreeMap::new(),
        zram_ratios: BTreeMap::new(),
        interval_seconds: Some(1.0),
        reliable,
    }
}

#[test]
fn test_five_phase_lifecycle_tier_migration() {
    const PAGE_SIZE: usize = 4096;
    const CAPACITY: usize = 16;
    const BUDGET: usize = 262144; // 256 KiB, ample for 16 raw pages

    let mut store = PageStore::adaptive(PAGE_SIZE, CAPACITY, BUDGET).unwrap();
    let mut oracle = Vec::new();

    // Insert 10 pages with distinct patterns
    for i in 0..10u8 {
        let mut page = vec![i; PAGE_SIZE];
        page[0] = i.wrapping_mul(7);
        page[PAGE_SIZE - 1] = i.wrapping_add(13);
        let id = store.insert(&page).unwrap();
        oracle.push((id, page));
    }

    let coordinator = AuraTieringCoordinator::new(AdaptiveTieringPolicy::new(750));

    // Phase 1: Low pressure -> all pages migrate to Memory tier (raw)
    let diag_low = make_diagnosis(vec!["normal"], Some(85.0), true);
    assert_eq!(
        PressureBridge::evaluate_pressure(&diag_low),
        MemoryPressure::Low
    );
    let rep_p1 = coordinator.step(&mut store, &diag_low).unwrap();
    assert_eq!(rep_p1.inspected_pages, 10);
    assert_eq!(rep_p1.migrated_to_memory, 10);
    for (id, _) in &oracle {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Memory);
    }
    let memory_accounted = store.stats().accounted_bytes;

    // Verify all pages read identically
    for (id, expected) in &oracle {
        assert_eq!(&store.get(*id).unwrap(), expected);
    }

    // Phase 2: Moderate pressure -> access first 3 pages to make them hot
    for (id, _) in &oracle[..3] {
        let _ = store.get(*id).unwrap();
    }
    let diag_mod = make_diagnosis(vec!["elevated_pressure"], Some(30.0), true);
    assert_eq!(
        PressureBridge::evaluate_pressure(&diag_mod),
        MemoryPressure::Moderate
    );
    let rep_p2 = coordinator.step(&mut store, &diag_mod).unwrap();
    assert_eq!(rep_p2.inspected_pages, 10);
    assert_eq!(rep_p2.migrated_to_compressed, 7);
    assert_eq!(rep_p2.no_change, 3);
    // Cold pages (latency > 750) migrate to Compressed; hot pages stay Memory
    for (id, _) in &oracle[..3] {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Memory);
    }
    for (id, _) in &oracle[3..] {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Compressed);
    }

    // Phase 3: High pressure -> all pages compressed
    let diag_high = make_diagnosis(vec!["sustained_pressure"], Some(12.0), true);
    assert_eq!(
        PressureBridge::evaluate_pressure(&diag_high),
        MemoryPressure::High
    );
    let rep_p3 = coordinator.step(&mut store, &diag_high).unwrap();
    assert_eq!(rep_p3.inspected_pages, 10);
    assert_eq!(rep_p3.migrated_to_compressed, 3);
    for (id, _) in &oracle {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Compressed);
    }
    let compressed_accounted = store.stats().accounted_bytes;
    assert!(compressed_accounted < memory_accounted);

    // Phase 4: Critical pressure -> strictly maintains Compressed tier
    let diag_crit = make_diagnosis(vec!["swap_thrashing"], Some(3.0), true);
    assert_eq!(
        PressureBridge::evaluate_pressure(&diag_crit),
        MemoryPressure::Critical
    );
    let rep_p4 = coordinator.step(&mut store, &diag_crit).unwrap();
    assert_eq!(rep_p4.inspected_pages, 10);
    assert_eq!(rep_p4.no_change, 10);

    // Phase 5: Pressure drops back to Low -> all pages safely decompress to Memory
    let rep_p5 = coordinator.step(&mut store, &diag_low).unwrap();
    assert_eq!(rep_p5.inspected_pages, 10);
    assert_eq!(rep_p5.migrated_to_memory, 10);
    for (id, _) in &oracle {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Memory);
    }
    assert_eq!(store.stats().accounted_bytes, memory_accounted);

    // Final full readback verification
    let mut buf = vec![0u8; PAGE_SIZE];
    for (id, expected) in &oracle {
        let n = store.get_into(*id, &mut buf).unwrap();
        assert_eq!(n, PAGE_SIZE);
        assert_eq!(&buf, expected);
    }
}

#[test]
fn test_unreliable_telemetry_safe_fallback() {
    let mut store = PageStore::adaptive(4096, 4, 65536).unwrap();
    let p = store.insert(&[0x77u8; 4096]).unwrap();

    let coordinator = AuraTieringCoordinator::default();

    // With stale/unreliable diagnosis, bridge must fall back to Moderate
    let diag_stale = make_diagnosis(vec!["swap_thrashing"], Some(2.0), false);
    assert_eq!(
        PressureBridge::evaluate_pressure(&diag_stale),
        MemoryPressure::Moderate
    );

    let rep = coordinator.step(&mut store, &diag_stale).unwrap();
    assert_eq!(rep.inspected_pages, 1);
    // Page was inserted into Compressed tier; under Moderate cold it stays Compressed
    assert_eq!(store.page_tier(p).unwrap(), StorageTier::Compressed);
    assert_eq!(store.get(p).unwrap(), vec![0x77u8; 4096]);
}

#[test]
fn test_budget_exhaustion_on_tier_expansion() {
    // Capacity 4, but budget only fits 2 raw pages (~8350 bytes)
    const BUDGET: usize = 9000;
    let mut store = PageStore::adaptive(4096, 4, BUDGET).unwrap();

    let mut ids = Vec::new();
    for i in 0..4u8 {
        ids.push(store.insert(&vec![i; 4096]).unwrap());
    }

    // All pages start Compressed (each ~100 bytes), total accounted ~400 bytes <= 9000
    for id in &ids {
        assert_eq!(store.page_tier(*id).unwrap(), StorageTier::Compressed);
    }

    let coordinator = AuraTieringCoordinator::default();
    let diag_low = make_diagnosis(vec!["normal"], Some(90.0), true);

    // Under Low pressure, coordinator attempts to migrate all to Memory (raw).
    // Each raw page costs ~4176 bytes.
    // 2 pages cost ~8352 bytes <= 9000.
    // The 3rd page would require 8352 + 4096 > 9000 -> BudgetExceeded!
    let rep = coordinator.step(&mut store, &diag_low).unwrap();
    assert_eq!(rep.inspected_pages, 4);
    assert_eq!(rep.migrated_to_memory, 2);
    assert_eq!(rep.no_change, 2); // 2 pages safely stayed Compressed!

    // Verify invariants: accounted bytes never exceeded budget!
    assert!(store.stats().accounted_bytes <= store.stats().budget_bytes);

    // Verify all 4 pages are readable and uncorrupted!
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(store.get(*id).unwrap(), vec![i as u8; 4096]);
    }
}
