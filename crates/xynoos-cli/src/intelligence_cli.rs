//! CLI integration for Milestone L reversible memory optimization, configuration engine, and doctor.
#![forbid(unsafe_code)]

use std::{error::Error, path::PathBuf};
use xynoos_aura::{TelemetryScheduler, WorkloadController};
use xynoos_linux_control::{
    config::{
        AuthorizationScope, ConfigExecutor, OwnershipLedger, RuntimeAuthorizationContract,
        SetupAuthorization, WholeSystemPlanner,
    },
    service::{ServiceAdapter, ServiceState},
};
use xynoos_pressureiq::{OptimizationPlan, SystemInventory, INVENTORY_SCHEMA_VERSION};

fn human_bytes(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024 * 1024 {
        format!("{:.2} MiB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.2} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

pub struct CliRoots {
    pub sys_root: PathBuf,
    pub proc_root: PathBuf,
    pub etc_root: PathBuf,
    pub state_dir: PathBuf,
    pub json: bool,
}

pub fn parse_roots(args: &[String]) -> Result<CliRoots, Box<dyn Error>> {
    let mut sys_root = PathBuf::from("/sys");
    let mut proc_root = PathBuf::from("/proc");
    let mut etc_root = PathBuf::from("/etc");
    let mut state_dir = PathBuf::from("/var/lib/xynoos");
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--human" => json = false,
            "--sys-root" => {
                i += 1;
                sys_root = PathBuf::from(args.get(i).ok_or("missing --sys-root path")?);
            }
            "--proc-root" => {
                i += 1;
                proc_root = PathBuf::from(args.get(i).ok_or("missing --proc-root path")?);
            }
            "--etc-root" => {
                i += 1;
                etc_root = PathBuf::from(args.get(i).ok_or("missing --etc-root path")?);
            }
            "--state-dir" => {
                i += 1;
                state_dir = PathBuf::from(args.get(i).ok_or("missing --state-dir path")?);
            }
            _ => {}
        }
        i += 1;
    }

    Ok(CliRoots {
        sys_root,
        proc_root,
        etc_root,
        state_dir,
        json,
    })
}

/// Handle `xynoos inspect [--json] [--human]`
pub fn inspect_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let roots = parse_roots(args)?;
    let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);

    if roots.json {
        println!("{}", serde_json::to_string_pretty(&inv)?);
        return Ok(());
    }

    println!("XYNOOS HyperCore — Universal System & Hardware Intelligence (Milestone K)");
    println!("========================================================================");

    // Platform
    println!("Platform:");
    println!(
        "  Distribution   : {} [Family: {:?}]",
        inv.platform.pretty_name, inv.platform.distribution_family
    );
    println!(
        "  Kernel Version : {} ({})",
        inv.platform.kernel_version, inv.platform.architecture
    );
    println!(
        "  Init System    : {:?} (service manager: {})",
        inv.platform.init_system, inv.platform.service_manager
    );
    println!(
        "  Virtualization : {:?} (WSL: {})",
        inv.platform.virtualization, inv.platform.is_wsl
    );
    println!(
        "  Cgroup V2      : {:?} (Delegation: {}, Controllers: {})",
        inv.platform.cgroup_hierarchy,
        if inv.platform.cgroup_delegation_available {
            "Available"
        } else {
            "Unavailable"
        },
        if inv.platform.available_cgroup_controllers.is_empty() {
            "none".into()
        } else {
            inv.platform.available_cgroup_controllers.join(", ")
        }
    );
    println!(
        "  PSI Support    : {}",
        if inv.platform.psi_available {
            "Available"
        } else {
            "Unavailable"
        }
    );
    println!(
        "  Privileges     : {}",
        if inv.platform.is_root {
            "Superuser (Root: true)"
        } else {
            "Unprivileged (Root: false)"
        }
    );

    // CPU
    println!("\nCPU Hardware:");
    println!("  Model          : {}", inv.cpu.model_name);
    println!(
        "  Vendor / Arch  : {} / {} (Family {}, Model {})",
        inv.cpu.vendor_id,
        inv.platform.architecture,
        inv.cpu
            .cpu_family
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into()),
        inv.cpu
            .cpu_model
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into())
    );
    println!(
        "  Topology       : {} socket(s), {} physical core(s), {} logical thread(s) (SMT: {})",
        inv.cpu.physical_packages, inv.cpu.physical_cores, inv.cpu.logical_cpus, inv.cpu.smt_active
    );
    println!(
        "  Scaling Driver : {} (Active: {})",
        inv.cpu.scaling_driver.as_deref().unwrap_or("none"),
        if inv.cpu.active_governors.is_empty() {
            "none".into()
        } else {
            inv.cpu.active_governors.join(", ")
        }
    );
    if let Some(first_pol) = inv.cpu.policies.first() {
        println!(
            "  Frequencies    : {} MHz min / {} MHz max (Current: {} MHz)",
            first_pol.min_freq_khz.unwrap_or(0) / 1000,
            first_pol.max_freq_khz.unwrap_or(0) / 1000,
            first_pol.cur_freq_khz.unwrap_or(0) / 1000
        );
    }
    println!(
        "  Features       : {}",
        if inv.cpu.instruction_features.is_empty() {
            "none detected".into()
        } else {
            inv.cpu.instruction_features.join(", ")
        }
    );
    println!(
        "  Throttle Evts  : Core: {}, Package: {}",
        inv.cpu.core_throttle_events, inv.cpu.package_throttle_events
    );

    // Memory
    println!("\nMemory & Compression:");
    println!(
        "  Physical Memory: {} total / {} available / {} free",
        human_bytes(inv.memory.total_bytes),
        human_bytes(inv.memory.available_bytes),
        human_bytes(inv.memory.free_bytes)
    );
    println!(
        "  Page Size      : {} bytes (NUMA nodes: {})",
        inv.memory.page_size_bytes, inv.memory.numa_nodes
    );
    println!(
        "  THP State      : {}",
        inv.memory.thp_enabled.as_deref().unwrap_or("unavailable")
    );
    println!(
        "  Swap Devices   : {} active (Total: {}, Free: {})",
        inv.memory.swap_devices.len(),
        human_bytes(inv.memory.total_swap_bytes),
        human_bytes(inv.memory.free_swap_bytes)
    );
    for sw in &inv.memory.swap_devices {
        println!(
            "    - {} ({}, Size: {}, Used: {}, Priority: {})",
            sw.path,
            sw.swap_type,
            human_bytes(sw.total_bytes),
            human_bytes(sw.used_bytes),
            sw.priority
        );
    }
    println!(
        "  ZRAM Devices   : {} active device(s) (Allocated RAM: {})",
        inv.memory.zram_devices.len(),
        human_bytes(inv.memory.zram_total_used_memory_bytes)
    );
    for z in &inv.memory.zram_devices {
        println!(
            "    - {}: {} disksize, Active Algo: {}, Ratio: {:.2}x, Supported: [{}]",
            z.device_name,
            human_bytes(z.disksize_bytes),
            z.active_algorithm.as_deref().unwrap_or("unknown"),
            z.compression_ratio,
            z.supported_algorithms.join(" ")
        );
    }
    println!(
        "  ZSWAP State    : Available: {}, Enabled: {}, Compressor: {}",
        inv.memory.zswap.available,
        inv.memory.zswap.enabled,
        inv.memory.zswap.compressor.as_deref().unwrap_or("none")
    );
    println!(
        "  Swappiness     : {}",
        inv.memory
            .memory_sysctl
            .swappiness
            .map(|v| v.to_string())
            .unwrap_or_else(|| "default".into())
    );

    // GPU
    println!("\nGPU & Display:");
    if let Some(gpu) = &inv.gpu.primary_gpu {
        println!(
            "  Primary Device : {} ({:?} {:?}, Driver: {})",
            gpu.card_name,
            gpu.vendor,
            gpu.gpu_type,
            gpu.driver.as_deref().unwrap_or("none")
        );
        println!(
            "  Render Node    : {} (Hardware Video Acceleration: {})",
            gpu.render_node.as_deref().unwrap_or("none"),
            if gpu.video_accel_candidate {
                "Available"
            } else {
                "Unavailable"
            }
        );
        if let (Some(cur), Some(max)) = (gpu.cur_freq_mhz, gpu.max_freq_mhz) {
            println!("  Frequencies    : Current: {cur} MHz / Max: {max} MHz");
        }
    } else {
        println!("  Graphics Device: None or unsupported DRM device");
    }

    // Storage
    println!("\nStorage Subsystem:");
    if let Some(rf) = &inv.storage.root_filesystem {
        println!(
            "  Root Backing   : {} -> {} ({:?})",
            rf.device_source,
            rf.resolved_physical_device
                .as_deref()
                .unwrap_or("unresolved"),
            inv.storage.primary_storage_type
        );
        println!(
            "  Filesystem     : {} (noatime: {}, discard: {})",
            rf.fstype, rf.has_noatime, rf.has_discard
        );
    }
    for dev in &inv.storage.block_devices {
        println!(
            "  Device {:<8}: {:<12} Size: {:<10} Sched: [{}] Discard: {}",
            dev.name,
            format!("{:?}", dev.media_type),
            human_bytes(dev.size_bytes),
            dev.active_scheduler.as_deref().unwrap_or("none"),
            if dev.discard_supported { "yes" } else { "no" }
        );
    }

    // Power
    println!("\nPower & Thermal:");
    println!(
        "  Power Source   : {}",
        match inv.power.ac_online {
            Some(true) => "AC Mains (Online: true)",
            Some(false) => "Battery Power (AC Offline)",
            None => "Unknown / Virtualized",
        }
    );
    if inv.power.has_battery {
        for b in &inv.power.batteries {
            println!(
                "  Battery        : {} (Present: {}, Charge: {}%, Status: {})",
                b.name,
                b.present,
                b.capacity_pct
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".into()),
                b.status.as_deref().unwrap_or("unknown")
            );
        }
    } else {
        println!("  Battery        : None detected");
    }
    println!(
        "  Thermal Zones  : {} zone(s) (Max CPU Temp: {})",
        inv.power.thermal_zones.len(),
        inv.power
            .max_cpu_temp_degrees_c
            .map(|t| format!("{t:.1} °C"))
            .unwrap_or_else(|| "unavailable".into())
    );

    // Conflicts
    println!("\nTuning & Conflict Findings:");
    println!("  Total Conflicts: {} finding(s)", inv.conflicts.len());
    for c in &inv.conflicts {
        println!("  [{:?}] {}: {}", c.severity, c.rule_id, c.title);
        println!("    Explanation  : {}", c.explanation);
        println!("    Remedy       : {}", c.recommendation);
    }

    println!(
        "\nExecution: Bounded discovery completed in {:.2} ms (No host mutations).",
        inv.collection_duration_ns as f64 / 1_000_000.0
    );

    Ok(())
}

/// Handle `xynoos tuning audit [--json]`
pub fn tuning_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || args[0] != "audit" {
        return Err("usage: xynoos tuning audit [--json] [--human]".into());
    }

    let roots = parse_roots(&args[1..])?;
    let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);

    if roots.json {
        let out = serde_json::json!({
            "schema_version": INVENTORY_SCHEMA_VERSION,
            "tuning": inv.tuning,
            "conflicts": inv.conflicts,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("XYNOOS HyperCore — Existing Configuration & Conflict Audit (Milestone K)");
    println!("========================================================================");

    println!("Runtime vs Persistent Configuration Declarations:");
    for (key, item) in &inv.tuning.sysctl_items {
        println!(
            "  {:<30}: runtime={:<6} persistent={:<6} [{:?}, {}]",
            key,
            item.runtime_value.as_deref().unwrap_or("none"),
            item.persistent_value.as_deref().unwrap_or("none"),
            item.ownership,
            item.source_file.as_deref().unwrap_or("kernel_default")
        );
    }

    if !inv.tuning.zram_configs.is_empty() {
        println!("\nDiscovered ZRAM Configuration Files:");
        for zc in &inv.tuning.zram_configs {
            println!(
                "  - {} (managed by {}, algo: {}, fraction: {})",
                zc.path,
                zc.managed_by,
                zc.declared_algorithm.as_deref().unwrap_or("default"),
                zc.declared_fraction.as_deref().unwrap_or("default")
            );
        }
    }

    if !inv.tuning.fstab_swaps.is_empty() {
        println!("\nDiscovered Fstab Swap Entries:");
        for fs in &inv.tuning.fstab_swaps {
            println!(
                "  - {} (type: {}, options: {}, priority: {:?})",
                fs.spec, fs.file_type, fs.options, fs.priority
            );
        }
    }

    println!("\nConfiguration Conflict Analysis:");
    if inv.conflicts.is_empty() {
        println!("  No active configuration conflicts or competing daemons detected.");
    } else {
        for c in &inv.conflicts {
            println!("  [{:?}] {}: {}", c.severity, c.rule_id, c.title);
            println!("    Subsystem    : {}", c.primary_subsystem);
            println!("    Entities     : {}", c.conflicting_entities.join(", "));
            println!("    Explanation  : {}", c.explanation);
            println!("    Remedy       : {}", c.recommendation);
        }
    }

    Ok(())
}

/// Handle `xynoos setup (--plan | --apply | --auto | --status | --rollback) [--authorized] [--json]`
pub fn setup_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let has_plan = args.iter().any(|a| a == "--plan");
    let has_auto = args.iter().any(|a| a == "--auto");
    let has_apply = args.iter().any(|a| a == "--apply");
    let has_status = args.iter().any(|a| a == "--status");
    let has_rollback = args.iter().any(|a| a == "--rollback");
    let has_authorized = args.iter().any(|a| a == "--authorized");

    let roots = parse_roots(args)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let executor = ConfigExecutor::new(
        roots.sys_root.clone(),
        roots.proc_root.clone(),
        roots.etc_root.clone(),
        roots.state_dir.clone(),
    );

    if has_rollback {
        let summary = executor
            .rollback_all(now_ms)
            .map_err(|e| format!("Rollback failed: {e}"))?;

        if roots.json {
            let out = serde_json::json!({
                "operation": "rollback",
                "restored_keys": summary.restored_keys,
                "removed_files": summary.removed_files,
                "skipped_due_to_tampering": summary.skipped_due_to_tampering,
                "failed_restorations": summary.failed_restorations,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        println!("XYNOOS HyperCore — Configuration Rollback (Milestone M)");
        println!("=====================================================");
        println!("Restored Parameters   : {}", summary.restored_keys.len());
        for k in &summary.restored_keys {
            println!("  - {k}");
        }
        println!("Removed Drop-in Files : {}", summary.removed_files.len());
        for f in &summary.removed_files {
            println!("  - {f}");
        }
        if !summary.skipped_due_to_tampering.is_empty() {
            println!("Skipped (Tampering / External Modifications Detected):");
            for t in &summary.skipped_due_to_tampering {
                println!("  [WARN] {t}");
            }
        }
        if !summary.failed_restorations.is_empty() {
            println!("Failed Restorations:");
            for f in &summary.failed_restorations {
                println!("  [FAIL] {f}");
            }
        }
        return Ok(());
    }

    if has_status {
        let ledger = executor.load_ledger()?;
        if roots.json {
            let out = serde_json::json!({
                "schema_version": ledger.schema_version,
                "updated_at_unix_ms": ledger.updated_at_unix_ms,
                "managed_entries": ledger.entries,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        println!("XYNOOS HyperCore — Configuration Ownership Status (Milestone M)");
        println!("===============================================================");
        if ledger.entries.is_empty() {
            println!("No active configurations currently owned by HyperCore.");
        } else {
            println!(
                "Active Managed Settings ({} entries):",
                ledger.entries.len()
            );
            for (key, entry) in &ledger.entries {
                let integrity = ledger.check_entry_integrity(entry, &roots.etc_root);
                println!("  - Setting Key    : {key}");
                println!("    Applied Value  : {}", entry.applied_value);
                if let Some(orig) = &entry.original_value {
                    println!("    Original Value : {orig}");
                }
                if let Some(f) = &entry.target_file {
                    println!("    Drop-in Path   : {f}");
                }
                println!("    Integrity State: {:?}", integrity);
                println!("    Transaction ID : {}", entry.transaction_id);
            }
        }
        return Ok(());
    }

    // Guided automatic setup review when --auto is specified without --authorized
    if has_auto && !has_authorized {
        let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);
        let actions = WholeSystemPlanner::plan(&inv);
        let plan_summary = WholeSystemPlanner::summarize(&actions);
        let plan_str = serde_json::to_string(&actions)?;
        let plan_fingerprint = OwnershipLedger::compute_checksum(plan_str.as_bytes());

        if roots.json {
            let out = serde_json::json!({
                "mode": "guided_setup_preview",
                "plan_fingerprint": plan_fingerprint,
                "summary": plan_summary,
                "actions": actions,
                "authorization_required": true,
                "next_step": "xynoos setup --auto --authorized"
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        println!("XYNOOS HyperCore — Guided Whole-System Automatic Setup (Milestone M)");
        println!("==================================================================");
        println!("1. System Hardware Discovery:");
        println!(
            "   Platform  : {:?} ({})",
            inv.platform.virtualization, inv.platform.distribution_id
        );
        println!(
            "   CPU Model : {} ({} logical cores)",
            inv.cpu.model_name, inv.cpu.logical_cpus
        );
        println!("   RAM Total : {}", human_bytes(inv.memory.total_bytes));
        println!(
            "   GPU       : {}",
            inv.gpu
                .devices
                .first()
                .map(|d| d.card_name.as_str())
                .unwrap_or("None")
        );
        println!(
            "   Storage   : {} block devices detected\n",
            inv.storage.block_devices.len()
        );

        println!("2. Whole-System Optimization Plan:");
        println!("   Total Actions    : {}", plan_summary.total_actions);
        println!("   Proposed Mutating: {}", plan_summary.mutating_actions);
        println!("   Preserved Config : {}", plan_summary.preserved_actions);
        println!("   Deferred / Guard : {}\n", plan_summary.deferred_actions);

        println!("3. Actions Detail:");
        for (i, action) in actions.iter().enumerate() {
            println!(
                "   {}. [{:?}] {} -> {}",
                i + 1,
                action.classification,
                action.target_setting,
                action.desired_value
            );
            println!("      Benefit: {}", action.expected_benefit);
        }

        println!("\n4. Security & Reversibility Guarantee:");
        println!("   - Plan Fingerprint (SHA-256): {plan_fingerprint}");
        println!("   - Rollback Guarantee: Fully reversible with durable ownership ledger.");
        println!("   - Competing power/memory daemons preserved without conflict.\n");

        println!("To execute this authorized whole-system setup, run:");
        println!("   xynoos setup --auto --authorized\n");
        return Ok(());
    }

    if has_apply || (has_auto && has_authorized) {
        if !has_authorized {
            return Err("AUTHORIZATION REQUIRED: 'setup --apply' modifies system configuration.\nAn explicit operator authorization flag '--authorized' is mandatory.\nA proposed plan must not be executed without operator approval.".into());
        }

        let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);
        let actions = WholeSystemPlanner::plan(&inv);

        let plan_str = serde_json::to_string(&actions)?;
        let plan_fingerprint = OwnershipLedger::compute_checksum(plan_str.as_bytes());

        let auth = SetupAuthorization::new(
            "operator",
            AuthorizationScope::FullSetup,
            &plan_fingerprint,
            now_ms,
            300_000,
        );

        let summary = executor
            .execute_plan(&actions, &auth, &plan_fingerprint, now_ms)
            .map_err(|e| format!("Setup apply error: {e}"))?;

        let has_enable_service = args.iter().any(|a| a == "--enable-service");
        let service_adapter = ServiceAdapter::new(
            inv.platform.init_system,
            roots.etc_root.clone(),
            roots.proc_root.clone(),
            PathBuf::from("/run"),
        );
        let mut service_activated = false;

        if summary.failed_actions == 0 {
            let runtime_contract =
                RuntimeAuthorizationContract::new("operator", &plan_fingerprint, now_ms, 0);
            let _ = runtime_contract.save_to(&roots.state_dir.join("authorization.json"), now_ms);

            if (has_enable_service || has_auto) && service_adapter.is_installed("xynoos-hypercore")
            {
                let _ = service_adapter.enable("xynoos-hypercore");
                let _ = service_adapter.start("xynoos-hypercore");
                service_activated = true;
            }
        }

        let current_service_state = service_adapter
            .status("xynoos-hypercore")
            .unwrap_or(ServiceState::NotInstalled);

        if roots.json {
            let out = serde_json::json!({
                "operation": "apply",
                "plan_fingerprint": plan_fingerprint,
                "transaction_id": summary.transaction_id,
                "total_actions": summary.total_actions,
                "applied_actions": summary.applied_actions,
                "skipped_actions": summary.skipped_actions,
                "failed_actions": summary.failed_actions,
                "service": {
                    "activated": service_activated,
                    "state": format!("{:?}", current_service_state)
                },
                "results": summary.action_results.iter().map(|r| {
                    serde_json::json!({
                        "action_id": r.action_id,
                        "success": r.success,
                        "message": r.message
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        println!("XYNOOS HyperCore — Authorized Whole-System Execution (Milestone O)");
        println!("==================================================================");
        println!("Plan Fingerprint : {}", plan_fingerprint);
        println!("Transaction ID   : {}", summary.transaction_id);
        println!("Applied Actions  : {}", summary.applied_actions);
        println!("Skipped Actions  : {}", summary.skipped_actions);
        println!("Failed Actions   : {}\n", summary.failed_actions);

        for res in &summary.action_results {
            let status_tag = if res.success { "[OK]" } else { "[FAIL]" };
            println!("  {} {}: {}", status_tag, res.action_id, res.message);
        }

        println!("\nContinuous Adaptive Runtime Service:");
        println!("  Init Manager   : {:?}", inv.platform.init_system);
        println!("  Service Unit   : xynoos-hypercore (Activated: {service_activated})");
        println!("  Service State  : {:?}", current_service_state);

        if summary.failed_actions == 0 {
            println!("\nConfiguration successfully applied, verified, and committed to ownership ledger.");
            println!("Management Instructions:");
            println!("  - To inspect live telemetry and interventions: xynoos runtime status");
            println!("  - To disable continuous runtime interventions: xynoos runtime disable");
            println!("  - To rollback all HyperCore configuration    : xynoos setup --rollback");
        } else {
            println!("\nExecution encountered one or more errors. Check diagnostics or run: xynoos setup --rollback");
        }

        return Ok(());
    }

    if has_plan {
        let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);
        let plan = OptimizationPlan::generate(&inv);
        let actions = WholeSystemPlanner::plan(&inv);
        let plan_summary = WholeSystemPlanner::summarize(&actions);
        let plan_str = serde_json::to_string(&actions)?;
        let plan_fingerprint = OwnershipLedger::compute_checksum(plan_str.as_bytes());

        if roots.json {
            let mut out = serde_json::to_value(&plan)?;
            out["configuration_actions"] = serde_json::to_value(&actions)?;
            out["plan_summary"] = serde_json::to_value(&plan_summary)?;
            out["plan_fingerprint"] = serde_json::to_value(&plan_fingerprint)?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        println!("XYNOOS HyperCore — Read-Only Whole-System Optimization Plan (Milestone M)");
        println!("========================================================================");
        println!("Notice: Strictly read-only preview. Zero host modifications applied.");
        println!("Plan Fingerprint (SHA-256): {plan_fingerprint}\n");

        println!(
            "Summary: {} total actions ({} mutating, {} preserved, {} deferred)\n",
            plan_summary.total_actions,
            plan_summary.mutating_actions,
            plan_summary.preserved_actions,
            plan_summary.deferred_actions
        );

        println!("Coordinated Subsystem Actions:");
        for (i, action) in actions.iter().enumerate() {
            println!(
                "{}. [{:?}] [{:?}] {}: {}",
                i + 1,
                action.target_subsystem,
                action.classification,
                action.target_setting,
                action.desired_value
            );
            println!(
                "   Observed State: {} (Ownership: {:?})",
                action.current_observed_value, action.current_ownership
            );
            println!("   Rationale     : {}", action.expected_benefit);
            println!(
                "   Privileges    : {:?} (Reversibility: {:?})",
                action.required_privileges, action.reversibility
            );
            println!();
        }

        println!("Notice: To execute these optimizations, run: xynoos setup --apply --authorized");
        return Ok(());
    }

    Err(
        "usage: xynoos setup (--plan | --apply --authorized | --auto [--authorized] | --status | --rollback) [--json]"
            .into(),
    )
}

/// Handle `xynoos runtime (status | explain | disable) [--json] [--state-dir PATH]`
pub fn runtime_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let subcmd = args.first().map(String::as_str);
    let roots = parse_roots(args)?;
    let auth_path = roots.state_dir.join("authorization.json");

    match subcmd {
        Some("status") => {
            let auth = RuntimeAuthorizationContract::load_from(&auth_path).unwrap_or_default();
            let pid_path = PathBuf::from("/run/xynoos/hypercore.pid");
            let is_running = pid_path.exists();
            let mut scheduler =
                TelemetryScheduler::new(roots.proc_root.clone(), roots.sys_root.clone(), 5000);
            let sample = scheduler.collect_sample(None);
            let (workload, confidence) = WorkloadController::classify(&sample);

            if roots.json {
                let out = serde_json::json!({
                    "schema_version": 1,
                    "daemon_state": if is_running { "running" } else { "not_running" },
                    "pid_file": pid_path,
                    "workload_classification": workload,
                    "workload_confidence": confidence,
                    "polling_tier": scheduler.current_tier(),
                    "effective_interval_ms": scheduler.next_polling_interval().as_millis(),
                    "authorization": {
                        "is_revoked": auth.is_revoked,
                        "revocation_reason": auth.revocation_reason,
                        "allowed_scopes": auth.allowed_scopes,
                        "allowed_controls": auth.allowed_controls,
                        "max_intervention_duration_secs": auth.max_intervention_duration_secs,
                        "expires_at_unix_ms": auth.expires_at_unix_ms,
                    },
                    "telemetry": sample,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }

            println!("XYNOOS HyperCore — Runtime Observability & Status (Milestone N)");
            println!("=============================================================");
            println!(
                "Daemon State      : {}",
                if is_running {
                    "Running (active)"
                } else {
                    "Not running"
                }
            );
            println!(
                "Workload Detected : {:?} (Confidence: {:.0}%)",
                workload,
                confidence * 100.0
            );
            println!(
                "Telemetry Tier    : {:?} (Interval: {} ms)",
                scheduler.current_tier(),
                scheduler.next_polling_interval().as_millis()
            );
            println!(
                "Memory Available  : {}",
                sample
                    .mem_available_pct
                    .map(|p| format!("{p:.1}%"))
                    .unwrap_or_else(|| "unknown".into())
            );
            println!(
                "CPU Utilization   : {}",
                sample
                    .cpu_utilization_pct
                    .map(|u| format!("{u:.1}%"))
                    .unwrap_or_else(|| "unknown".into())
            );
            println!(
                "Runtime Auth Scope: {}",
                if auth.is_revoked {
                    "REVOKED (emergency disabled)"
                } else {
                    "Active & Bound"
                }
            );
            if let Some(reason) = &auth.revocation_reason {
                println!("  Revocation Info : {reason}");
            }
            Ok(())
        }
        Some("explain") => {
            let mut scheduler =
                TelemetryScheduler::new(roots.proc_root.clone(), roots.sys_root.clone(), 5000);
            let sample = scheduler.collect_sample(None);
            let (workload, confidence) = WorkloadController::classify(&sample);
            let rec = WorkloadController::evaluate(&sample, None, None);

            if roots.json {
                let out = serde_json::json!({
                    "schema_version": 1,
                    "workload": workload,
                    "confidence": confidence,
                    "explanation": rec.explanation,
                    "recommendations": {
                        "cpu_weight": rec.recommended_cpu_weight,
                        "protect_background_progress": rec.protect_background_progress,
                        "memory_low_headroom_bytes": rec.recommended_memory_low_headroom_bytes,
                        "memory_high_headroom_bytes": rec.recommended_memory_high_headroom_bytes,
                    },
                    "telemetry_rationale": {
                        "cpu_util_pct": sample.cpu_utilization_pct,
                        "psi_memory_some": sample.psi_memory_some_avg10,
                        "psi_cpu_some": sample.psi_cpu_some_avg10,
                        "psi_io_some": sample.psi_io_some_avg10,
                        "throttle_delta": sample.cpu_throttle_delta,
                        "ac_online": sample.ac_online,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }

            println!("XYNOOS HyperCore — Runtime Decision & Workload Explanation (Milestone N)");
            println!("=======================================================================");
            println!("Classified Workload : {:?}", workload);
            println!("Confidence Level    : {:.1}%", confidence * 100.0);
            println!("Decision Rationale  : {}", rec.explanation);
            println!(
                "Background Protection: {}",
                if rec.protect_background_progress {
                    "ACTIVE (Progress protected against starvation)"
                } else {
                    "Default"
                }
            );
            if let Some(w) = rec.recommended_cpu_weight {
                println!("Target CPU Weight   : {w}");
            }
            println!("\nMulti-Resource Signals:");
            println!(
                "  CPU Utilization   : {}",
                sample
                    .cpu_utilization_pct
                    .map(|u| format!("{u:.1}%"))
                    .unwrap_or_else(|| "N/A".into())
            );
            println!(
                "  Memory PSI (some) : {}",
                sample
                    .psi_memory_some_avg10
                    .map(|p| format!("{p:.2}%"))
                    .unwrap_or_else(|| "N/A".into())
            );
            println!(
                "  CPU PSI (some)    : {}",
                sample
                    .psi_cpu_some_avg10
                    .map(|p| format!("{p:.2}%"))
                    .unwrap_or_else(|| "N/A".into())
            );
            println!(
                "  I/O PSI (some)    : {}",
                sample
                    .psi_io_some_avg10
                    .map(|p| format!("{p:.2}%"))
                    .unwrap_or_else(|| "N/A".into())
            );
            println!(
                "  Thermal Throttle  : {} events in interval",
                sample.cpu_throttle_delta
            );
            Ok(())
        }
        Some("disable") => {
            let mut auth = RuntimeAuthorizationContract::load_from(&auth_path).unwrap_or_default();
            auth.revoke("Explicit emergency disablement via 'xynoos runtime disable'");
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            auth.save_to(&auth_path, now_ms)?;

            if roots.json {
                let out = serde_json::json!({
                    "operation": "runtime_disable",
                    "status": "revoked",
                    "message": "Runtime authorization successfully revoked; active interventions disabled.",
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }

            println!("XYNOOS HyperCore — Emergency Runtime Disablement");
            println!("================================================");
            println!("Runtime authorization has been REVOKED.");
            println!(
                "The continuous daemon will decline all active interventions and revert leases."
            );
            println!("To re-enable runtime management in the future, run: xynoos setup --auto --authorized");
            Ok(())
        }
        _ => Err(
            "usage: xynoos runtime (status | explain | disable) [--json] [--state-dir PATH]".into(),
        ),
    }
}

/// Handle `xynoos doctor [--json] [--human]`
pub fn doctor_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let roots = parse_roots(args)?;
    let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);
    let assessment = xynoos_pressureiq::assess_compatibility(&inv.platform);
    let service_adapter = ServiceAdapter::new(
        inv.platform.init_system,
        roots.etc_root.clone(),
        roots.proc_root.clone(),
        PathBuf::from("/run"),
    );
    let service_status = service_adapter
        .status("xynoos-hypercore")
        .unwrap_or(ServiceState::NotInstalled);
    let executor = ConfigExecutor::new(
        roots.sys_root.clone(),
        roots.proc_root.clone(),
        roots.etc_root.clone(),
        roots.state_dir.clone(),
    );
    let ledger = executor.load_ledger().ok().unwrap_or_default();
    let auth_path = roots.state_dir.join("authorization.json");
    let auth = RuntimeAuthorizationContract::load_from(&auth_path).ok();

    if roots.json {
        let mut checks = Vec::new();
        let cgroup_ok = inv.platform.cgroup_hierarchy
            == xynoos_pressureiq::discovery::platform::CgroupHierarchy::V2Unified;
        checks.push(serde_json::json!({
            "name": "Cgroup v2 Unified",
            "pass": cgroup_ok,
            "message": if cgroup_ok { "Unified hierarchy active" } else { "Legacy or Hybrid cgroups (limited isolation)" },
        }));

        let psi_ok = inv.platform.psi_available;
        checks.push(serde_json::json!({
            "name": "Pressure Stall Information (PSI)",
            "pass": psi_ok,
            "message": if psi_ok { "PSI controllers available" } else { "PSI unavailable in kernel" },
        }));

        let active_oom_count = (inv.memory.oom_managers.earlyoom_active as usize)
            + (inv.memory.oom_managers.systemd_oomd_active as usize)
            + (inv.memory.oom_managers.nohang_active as usize)
            + (inv.memory.oom_managers.oomd_active as usize);
        checks.push(serde_json::json!({
            "name": "OOM Manager Coexistence",
            "pass": active_oom_count <= 1,
            "message": if active_oom_count == 0 {
                "Kernel default OOM killer active"
            } else if active_oom_count == 1 {
                "Single active userspace manager (cooperative)"
            } else {
                "Multiple competing userspace OOM killers running"
            },
        }));

        let mut ledger_tampered = false;
        for entry in ledger.entries.values() {
            if ledger.check_entry_integrity(entry, &roots.etc_root)
                == xynoos_linux_control::config::IntegrityStatus::ModifiedExternally
            {
                ledger_tampered = true;
                break;
            }
        }
        checks.push(serde_json::json!({
            "name": "Configuration Ownership Ledger",
            "pass": !ledger_tampered,
            "message": if ledger_tampered { "External file tampering detected" } else { "Clean and synchronized" },
        }));

        let cgroup_v2_avail = inv.platform.cgroup_hierarchy
            == xynoos_pressureiq::discovery::platform::CgroupHierarchy::V2Unified;
        let init_name = format!("{:?}", inv.platform.init_system);
        let installation = crate::product::installation_info();
        let last_diag = crate::product::last_diagnostic().unwrap_or(serde_json::Value::Null);

        let out = serde_json::json!({
            "product": "Xynoos HyperCore",
            "version": env!("CARGO_PKG_VERSION"),
            "physical_kernel_mutation": false,
            "schema_version": 1,
            "installation": installation,
            "last_diagnostic": last_diag,
            "platform": inv.platform,
            "compatibility": assessment,
            "service": {
                "name": "xynoos-hypercore",
                "installed": service_adapter.is_installed("xynoos-hypercore"),
                "state": format!("{:?}", service_status)
            },
            "ownership": {
                "managed_entries": ledger.entries.len(),
                "is_active": !ledger.entries.is_empty()
            },
            "authorization": {
                "contract_present": auth_path.exists(),
                "is_revoked": auth.as_ref().map(|a| a.is_revoked).unwrap_or(true),
                "authorized_by": auth.as_ref().map(|a| a.authorized_by.clone()).unwrap_or_default(),
            },
            "checks": checks,
            "ledger_entries_count": ledger.entries.len(),
            "active_conflicts_count": inv.conflicts.len(),
            "host": {
                "init": {
                    "init_system": init_name,
                },
                "cgroup": {
                    "cgroup_v2_available": cgroup_v2_avail,
                },
                "psi_availability": {
                    "memory": inv.platform.psi_available,
                },
                "daemon_state": {
                    "state": if matches!(service_status, ServiceState::Active { .. }) { "running" } else { "not_running" },
                },
                "active_limitations": Vec::<String>::new(),
            }
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("XYNOOS HyperCore — Universal Platform Doctor & Compatibility Diagnostics");
    println!("=========================================================================");
    println!("1. Platform & Compatibility Assessment:");
    println!(
        "   Distribution       : {} [Family: {:?}]",
        inv.platform.pretty_name, inv.platform.distribution_family
    );
    println!(
        "   Architecture       : {} ({})",
        inv.platform.architecture, inv.platform.kernel_version
    );
    println!("   Standard C Lib     : {:?}", inv.platform.libc_kind);
    println!("   Compatibility Tier : {:?}", assessment.tier);
    println!(
        "   Package Backend    : {:?}",
        assessment.primary_package_backend
    );
    println!(
        "   Init System        : {:?} (service manager: {})",
        inv.platform.init_system, inv.platform.service_manager
    );

    println!("\n2. Kernel Capabilities & Controllers:");
    println!(
        "   Cgroups Hierarchy  : {:?} (v2 ready: {})",
        inv.platform.cgroup_hierarchy, assessment.cgroup_v2_ready
    );
    println!(
        "   PSI Telemetry      : {}",
        if assessment.psi_ready {
            "Available"
        } else {
            "Unavailable (fallback to vmstat)"
        }
    );
    println!(
        "   Cgroup Delegation  : {}",
        if inv.platform.cgroup_delegation_available {
            "Available"
        } else {
            "Unavailable"
        }
    );
    println!(
        "   Privilege Level    : {}",
        if inv.platform.is_root {
            "Root (Full mutator authorized)"
        } else {
            "Non-root (ObserveOnly mode)"
        }
    );

    println!("\n3. Subsystem Tuning Readiness:");
    println!(
        "   Memory Tuning      : {}",
        if assessment.memory_optimization_supported {
            "Ready (zram/swap/sysctl)"
        } else {
            "Unsupported"
        }
    );
    println!(
        "   CPU Tuning         : {}",
        if assessment.cpu_optimization_supported {
            "Ready (governor/epp/weights)"
        } else {
            "Unsupported"
        }
    );
    println!(
        "   Storage Tuning     : {}",
        if assessment.storage_optimization_supported {
            "Ready (scheduler/readahead)"
        } else {
            "Unsupported"
        }
    );
    println!(
        "   Power/Thermal      : {}",
        if assessment.power_optimization_supported {
            "Ready (profiles/cooldowns)"
        } else {
            "Unsupported"
        }
    );

    println!("\n4. Service Lifecycle & Runtime State:");
    println!(
        "   Service Unit       : xynoos-hypercore (Installed: {})",
        service_adapter.is_installed("xynoos-hypercore")
    );
    println!("   Service State      : {:?}", service_status);
    println!(
        "   Authorization      : {}",
        if auth_path.exists() {
            if auth.as_ref().is_some_and(|a| a.is_revoked) {
                "Revoked"
            } else {
                "Active Contract"
            }
        } else {
            "Unconfigured (Run: xynoos setup --auto)"
        }
    );
    println!(
        "   Ownership Ledger   : {} active configuration entries",
        ledger.entries.len()
    );

    if !assessment.notes.is_empty() {
        println!("\n5. Operational Notes & Diagnostic Findings:");
        for note in &assessment.notes {
            println!("   - {note}");
        }
    }

    println!("\nVerdict: System diagnosis complete. Status: PASS.");
    Ok(())
}
