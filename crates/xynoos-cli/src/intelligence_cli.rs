//! CLI integration for Milestone K universal hardware intelligence, configuration audit, and setup planning.
#![forbid(unsafe_code)]

use std::{error::Error, path::PathBuf};
use xynoos_pressureiq::{
    OptimizationPlan, ProposedActionType, SystemInventory, INVENTORY_SCHEMA_VERSION,
};

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

struct CliRoots {
    sys_root: PathBuf,
    proc_root: PathBuf,
    etc_root: PathBuf,
    json: bool,
}

fn parse_roots(args: &[String]) -> Result<CliRoots, Box<dyn Error>> {
    let mut sys_root = PathBuf::from("/sys");
    let mut proc_root = PathBuf::from("/proc");
    let mut etc_root = PathBuf::from("/etc");
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
            _ => {}
        }
        i += 1;
    }

    Ok(CliRoots {
        sys_root,
        proc_root,
        etc_root,
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

/// Handle `xynoos setup --plan [--json]`
pub fn setup_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let has_plan = args.iter().any(|a| a == "--plan");
    let has_apply = args.iter().any(|a| a == "--apply" || a == "--auto");

    if has_apply {
        return Err(
            "ABORTED: 'setup --apply' is not permitted during Milestone K.\nMilestone K establishes the read-only discovery and planning foundation.\nUse 'xynoos setup --plan' or 'xynoos setup --plan --json'."
                .into(),
        );
    }

    if !has_plan {
        return Err("usage: xynoos setup --plan [--json] [--human]".into());
    }

    let roots = parse_roots(args)?;
    let inv = SystemInventory::collect_from(&roots.sys_root, &roots.proc_root, &roots.etc_root);
    let plan = OptimizationPlan::generate(&inv);

    if roots.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    println!("XYNOOS HyperCore — Read-Only Optimization Plan (Milestone K)");
    println!("===========================================================");
    println!("Notice: Strictly read-only planning foundation. No host modifications applied.\n");

    println!("Optimization Actions Summary:");
    println!("  Preserve Actions : {}", plan.summary_preserve_count);
    println!("  Configure Actions: {}", plan.summary_configure_count);
    println!("  Improve Actions  : {}", plan.summary_improve_count);
    println!("  Conflict Actions : {}", plan.summary_conflict_count);
    println!("  Unsupported      : {}", plan.summary_unsupported_count);
    println!("  Total Actions    : {}\n", plan.actions.len());

    println!("Proposed Actions:");
    for (i, action) in plan.actions.iter().enumerate() {
        let tag = match action.action_type {
            ProposedActionType::Preserve => "[PRESERVE]",
            ProposedActionType::Configure => "[CONFIGURE]",
            ProposedActionType::Improve => "[IMPROVE]",
            ProposedActionType::Defer => "[DEFER]",
            ProposedActionType::Unsupported => "[UNSUPPORTED]",
            ProposedActionType::Conflict => "[CONFLICT]",
            ProposedActionType::RequiresAuthorization => "[AUTH NEEDED]",
            ProposedActionType::InsufficientEvidence => "[INSUFFICIENT]",
        };

        println!(
            "{}. {} {:?}: {}",
            i + 1,
            tag,
            action.subsystem,
            action.target_parameter
        );
        println!(
            "   Current State : {} (Ownership: {:?})",
            action.current_state, action.detected_ownership
        );
        if let Some(prop) = &action.proposed_setting {
            println!("   Proposed Setting: {prop}");
        }
        println!("   Rationale     : {}", action.rationale);
        println!("   Expected Gain : {}", action.expected_benefit);
        println!(
            "   Privileges    : {:?} (Reversibility: {:?}, Confidence: {:?})",
            action.required_privileges, action.reversibility, action.confidence
        );
        println!();
    }

    println!("Notice: To execute actions in future milestones, explicit authorization and an operational milestone release will be required.");

    Ok(())
}
