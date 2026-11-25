#!/usr/bin/env python3
"""Reproducible Resource-Constrained Linux Benchmark Laboratory for XYNOOS HyperCore (Milestone H).

Executes controlled paired trials inside a delegated cgroup v2 parent across:
- Baseline A: Standard Linux (HyperCore disabled)
- Baseline B: Resource-optimized Linux (zram/swap when supported, HyperCore disabled)
- Candidate C: HyperCore ObserveOnly daemon
- Candidate D: HyperCore Advisory daemon
- Candidate E: HyperCore Transactional (explicitly authorized bounded SustainedLease on disposable workload cgroups)

Candidate F (HyperMemory-aware application storage) is measured separately via `xynoos-bench hypermemory-app-bench`.
NEVER run on the developer's physical laptop; intended exclusively for isolated GitHub Actions CI runners.
"""
import argparse
import csv
import json
import math
import os
from pathlib import Path
import random
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import http.server
import urllib.request


def read_text(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def parse_psi(path):
    txt = read_text(path)
    if not txt:
        return None
    out = {}
    for line in txt.splitlines():
        parts = line.split()
        if not parts:
            continue
        kind = parts[0]
        vals = {}
        for kv in parts[1:]:
            if "=" in kv:
                k, v = kv.split("=", 1)
                vals[k] = float(v) if k != "total" else int(v)
        out[kind] = vals
    return out


def parse_vmstat():
    txt = read_text("/proc/vmstat")
    if not txt:
        return {}
    res = {}
    for line in txt.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] in ("pswpin", "pswpout", "pgmajfault", "pgfault"):
            res[parts[0]] = int(parts[1])
    return res


def parse_cgroup_events(cg_dir):
    txt = read_text(Path(cg_dir) / "memory.events")
    res = {}
    if txt:
        for line in txt.splitlines():
            parts = line.split()
            if len(parts) == 2:
                res[parts[0]] = int(parts[1])
    return res


def parse_proc_metrics(pid):
    status = read_text(f"/proc/{pid}/status") or ""
    rss_kib = 0
    hwm_kib = 0
    vol_ctx = 0
    nonvol_ctx = 0
    for line in status.splitlines():
        if line.startswith("VmRSS:"):
            rss_kib = int(line.split()[1])
        elif line.startswith("VmHWM:"):
            hwm_kib = int(line.split()[1])
        elif line.startswith("voluntary_ctxt_switches:"):
            vol_ctx = int(line.split()[1])
        elif line.startswith("nonvoluntary_ctxt_switches:"):
            nonvol_ctx = int(line.split()[1])
    pss_kib = None
    smaps = read_text(f"/proc/{pid}/smaps_rollup")
    if smaps:
        for line in smaps.splitlines():
            if line.startswith("Pss:"):
                pss_kib = int(line.split()[1])
                break
    try:
        fd_count = len(os.listdir(f"/proc/{pid}/fd"))
    except OSError:
        fd_count = None
    return {
        "rss_kib": rss_kib,
        "peak_rss_kib": hwm_kib,
        "pss_kib": pss_kib,
        "voluntary_ctxt_switches": vol_ctx,
        "nonvoluntary_ctxt_switches": nonvol_ctx,
        "fd_count": fd_count,
    }


def write_cg(cg_dir, file_name, value):
    Path(cg_dir, file_name).write_text(str(value))


def move_pid_to_cgroup(cg_dir, pid):
    Path(cg_dir, "cgroup.procs").write_text(str(pid))


def spawn_in_cgroup(cg_dir, cmd, env=None):
    def preexec():
        move_pid_to_cgroup(cg_dir, os.getpid())

    return subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        preexec_fn=preexec,
        env=env,
    )


def summarize(values):
    if not values:
        return {"n": 0, "median": None, "mean": None, "stddev": None, "iqr": None, "ci95_half": None, "min": None, "max": None}
    s = sorted(values)
    n = len(s)
    med = statistics.median(s)
    mean = statistics.fmean(s)
    sd = statistics.stdev(s) if n > 1 else 0.0
    q1 = statistics.quantiles(s, n=4)[0] if n >= 4 else s[0]
    q3 = statistics.quantiles(s, n=4)[2] if n >= 4 else s[-1]
    ci95 = 2.776 * (sd / math.sqrt(n)) if n == 5 else (1.96 * sd / math.sqrt(n) if n > 1 else 0.0)
    return {
        "n": n,
        "median": round(med, 3),
        "mean": round(mean, 3),
        "stddev": round(sd, 3),
        "iqr": round(q3 - q1, 3),
        "ci95_half": round(ci95, 3),
        "min": round(s[0], 3),
        "max": round(s[-1], 3),
    }


def start_local_http_server():
    class QuietHandler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = b'<!doctype html><html><head><title>XYNOOS Bench</title></head><body><div id="out"></div><script>let s=0;for(let i=0;i<50000;i++){s+=i%17;}document.getElementById("out").textContent="ok:"+s;</script></body></html>'
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt, *args):
            return

    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), QuietHandler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    return srv, srv.server_address[1]


def run_single_trial(
    parent_cg,
    xynoos_bin,
    candidate,
    profile_cfg,
    workload_kind,
    http_port,
    work_dir,
):
    session_cg = Path(parent_cg) / f"bench-session-{os.getpid()}-{int(time.time() * 1000)}"
    fg_cg = session_cg / "fg"
    bg_cg = session_cg / "bg"
    session_cg.mkdir(parents=True, exist_ok=True)
    # Ensure memory & cpu controllers enabled on session_cg if available
    avail_ctrl = (read_text(Path(parent_cg) / "cgroup.controllers") or "").split()
    enable = [f"+{c}" for c in ("memory", "cpu") if c in avail_ctrl]
    if enable:
        try:
            write_cg(session_cg, "cgroup.subtree_control", " ".join(enable))
        except OSError:
            pass
    fg_cg.mkdir(exist_ok=True)
    bg_cg.mkdir(exist_ok=True)

    # Apply profile limits to session_cg (shared contention ceiling + CPU cap)
    try:
        write_cg(session_cg, "memory.max", str(profile_cfg["session_mem_bytes"]))
    except OSError:
        pass
    if "cpu" in avail_ctrl:
        try:
            write_cg(session_cg, "cpu.max", profile_cfg["cpu_max"])
        except OSError:
            pass

    daemon_proc = None
    daemon_out_fh = None
    lease_proc = None
    daemon_pid_file = Path(work_dir) / "daemon.pid"
    daemon_out = Path(work_dir) / "daemon.json"
    daemon_metrics = {}
    bench_bin = str(Path(xynoos_bin).with_name("xynoos-bench"))

    if candidate in ("candidate_c_observe_only", "candidate_d_advisory"):
        mode_flag = "observe-only" if candidate == "candidate_c_observe_only" else "advisory"
        daemon_out_fh = open(daemon_out, "w")
        daemon_proc = subprocess.Popen(
            [xynoos_bin, "daemon", "--mode", mode_flag, "--interval-ms", "200", "--pid-file", str(daemon_pid_file)],
            stdout=daemon_out_fh,
            stderr=subprocess.PIPE,
        )
        time.sleep(0.25)
    elif candidate == "candidate_e_transactional":
        # Explicitly authorized bounded policy via Rust WorkloadCgroup + SustainedLease:
        # protect foreground memory.low; only apply background memory.high when contended (not unpressured Profile C)
        # Note: xynoos-linux-control enforces MAX_LOW_BYTES = 64 MiB, MAX_HIGH_BYTES = 512 MiB
        target_fg_low = min(int(profile_cfg["session_mem_bytes"] * 0.45), 64 * 1024 * 1024)
        fg_low = target_fg_low // 4096 * 4096
        contended = profile_cfg["bg_alloc_mib"] * 1024 * 1024 >= int(profile_cfg["session_mem_bytes"] * 0.85)
        target_bg_high = min(int(profile_cfg["session_mem_bytes"] * 0.55), 512 * 1024 * 1024)
        bg_high = (target_bg_high // 4096 * 4096) if contended else 0
        lease_proc = subprocess.Popen(
            [bench_bin, "workload-lease", str(fg_cg), str(fg_low), str(bg_cg), str(bg_high)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        ready = lease_proc.stdout.readline().decode().strip()
        if ready != "READY":
            raise RuntimeError(f"workload-lease failed: {lease_proc.stderr.read().decode()}")

    vm_before = parse_vmstat()
    psi_mem_before = parse_psi("/proc/pressure/memory")
    psi_cpu_before = parse_psi("/proc/pressure/cpu")
    psi_io_before = parse_psi("/proc/pressure/io")
    t_wall_0 = time.monotonic()

    # Background pressure worker in bg_cg: allocates & touches real anonymous memory + file page churn
    bg_mb = profile_cfg["bg_alloc_mib"]
    bg_code = (
        "import time, os\n"
        f"chunks = []\n"
        f"for i in range({bg_mb}):\n"
        "    b = bytearray(1024 * 1024)\n"
        "    for k in range(0, len(b), 4096):\n"
        "        b[k] = (i + k) & 0xFF\n"
        "    chunks.append(b)\n"
        "    if i % 8 == 0:\n"
        "        time.sleep(0.01)\n"
        "for r in range(6):\n"
        "    for idx, b in enumerate(chunks):\n"
        "        b[0] = (b[0] + 1) & 0xFF\n"
        "        if idx % 16 == 0:\n"
        "            time.sleep(0.005)\n"
    )
    bg_proc = spawn_in_cgroup(bg_cg, [sys.executable, "-c", bg_code])

    # Foreground workload inside fg_cg
    fg_latencies_ms = []
    fg_wall_s = 0.0
    fg_ok = True
    browser_ms = None

    if workload_kind in ("interactive_probe", "mixed_multitask"):
        # Interactive probe: 60 bounded HTTP/compute requests while background pressure runs
        probe_code = (
            "import time, urllib.request, json\n"
            f"url = 'http://127.0.0.1:{http_port}/'\n"
            "lats = []\n"
            "for i in range(60):\n"
            "    t0 = time.monotonic()\n"
            "    with urllib.request.urlopen(url, timeout=5) as resp:\n"
            "        _ = resp.read()\n"
            "    # small working-set allocation touch\n"
            "    buf = bytearray(262144)\n"
            "    buf[0] = i & 0xFF\n"
            "    buf[-1] = i & 0xFF\n"
            "    lats.append((time.monotonic() - t0) * 1000.0)\n"
            "    time.sleep(0.015)\n"
            "print(json.dumps(lats))\n"
        )
        t_fg0 = time.monotonic()
        fg_proc = spawn_in_cgroup(fg_cg, [sys.executable, "-c", probe_code])

        comp_proc = None
        if workload_kind == "mixed_multitask":
            c_src = Path(work_dir) / "sample.c"
            c_out = Path(work_dir) / "sample.bin"
            c_src.write_text("#include <stdio.h>\nint main(){long s=0;for(long i=0;i<2000000;i++)s+=i%7;printf(\"%ld\\n\",s);return 0;}\n")
            if shutil.which("gcc"):
                comp_proc = spawn_in_cgroup(fg_cg, ["gcc", "-O2", str(c_src), "-o", str(c_out)])

        out, err = fg_proc.communicate(timeout=60)
        if fg_proc.returncode == 0:
            fg_latencies_ms = json.loads(out.decode().strip())
        else:
            fg_ok = False
        if comp_proc is not None:
            comp_proc.communicate(timeout=60)
            if comp_proc.returncode != 0:
                fg_ok = False
        fg_wall_s = time.monotonic() - t_fg0

    elif workload_kind == "dev_compile":
        # Small/medium C compilation workload in fg_cg under background memory pressure
        c_src = Path(work_dir) / "compile_unit.c"
        c_out = Path(work_dir) / "compile_unit.bin"
        lines = ["#include <stdio.h>\n"]
        for fn in range(1200):
            lines.append(f"__attribute__((noinline)) static int fn_{fn}(int x) {{ return ((x * {fn + 3}) ^ {fn * 17}) + (x >> 2); }}\n")
        lines.append("int main(void){ int acc=0; for(int i=0;i<1000;i++){ acc+=fn_0(i)+fn_1199(i); } printf(\"%d\\n\",acc); return 0; }\n")
        c_src.write_text("".join(lines))
        t_fg0 = time.monotonic()
        compiler = shutil.which("gcc") or shutil.which("cc")
        if compiler:
            fg_proc = spawn_in_cgroup(fg_cg, [compiler, "-O2", str(c_src), "-o", str(c_out)])
            fg_proc.communicate(timeout=60)
            fg_ok = fg_proc.returncode == 0
        else:
            fg_ok = False
        fg_wall_s = time.monotonic() - t_fg0

    elif workload_kind == "browser_headless":
        browser = shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser")
        if browser:
            t_fg0 = time.monotonic()
            for b_idx in range(3):
                b_profile = Path(work_dir) / f"chrome-prof-{b_idx}"
                b_profile.mkdir(parents=True, exist_ok=True)
                b0 = time.monotonic()
                bp = spawn_in_cgroup(
                    fg_cg,
                    [
                        browser,
                        "--headless",
                        "--no-sandbox",
                        "--disable-gpu",
                        f"--user-data-dir={b_profile}",
                        "--dump-dom",
                        f"http://127.0.0.1:{http_port}/",
                    ],
                )
                bp.communicate(timeout=45)
                if bp.returncode == 0:
                    fg_latencies_ms.append((time.monotonic() - b0) * 1000.0)
                else:
                    fg_ok = False
                shutil.rmtree(b_profile, ignore_errors=True)
            fg_wall_s = time.monotonic() - t_fg0
            browser_ms = statistics.median(fg_latencies_ms) if fg_latencies_ms else None
        else:
            fg_ok = False

    bg_out, bg_err = bg_proc.communicate(timeout=90)
    bg_ok = bg_proc.returncode == 0
    total_wall_s = time.monotonic() - t_wall_0

    if daemon_proc is not None:
        if daemon_proc.poll() is None:
            daemon_metrics = parse_proc_metrics(daemon_proc.pid)
            daemon_proc.send_signal(signal.SIGTERM)
            try:
                daemon_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon_proc.kill()
                daemon_proc.wait()
    if daemon_out_fh is not None:
        daemon_out_fh.close()

    if lease_proc is not None:
        try:
            lease_proc.stdin.write(b"DONE\n")
            lease_proc.stdin.flush()
            lease_proc.stdin.close()
        except OSError:
            pass
        lease_proc.communicate(timeout=10)

    fg_events = parse_cgroup_events(fg_cg)
    bg_events = parse_cgroup_events(bg_cg)
    fg_peak = int(read_text(fg_cg / "memory.peak") or 0)
    bg_peak = int(read_text(bg_cg / "memory.peak") or 0)

    vm_after = parse_vmstat()
    psi_mem_after = parse_psi("/proc/pressure/memory")
    psi_cpu_after = parse_psi("/proc/pressure/cpu")
    psi_io_after = parse_psi("/proc/pressure/io")

    # Cleanup cgroups
    for cg in (fg_cg, bg_cg, session_cg):
        try:
            cg.rmdir()
        except OSError:
            pass

    def delta_psi(before, after):
        if not before or not after:
            return None
        return max(0, after.get("some", {}).get("total", 0) - before.get("some", {}).get("total", 0))

    lats_sorted = sorted(fg_latencies_ms)
    p50 = statistics.median(lats_sorted) if lats_sorted else None
    p95 = lats_sorted[int(len(lats_sorted) * 0.95)] if lats_sorted else None
    p99 = lats_sorted[min(len(lats_sorted) - 1, int(len(lats_sorted) * 0.99))] if lats_sorted else None
    max_stall = lats_sorted[-1] if lats_sorted else None

    return {
        "candidate": candidate,
        "workload": workload_kind,
        "fg_ok": fg_ok,
        "bg_ok": bg_ok,
        "total_wall_ms": round(total_wall_s * 1000.0, 2),
        "fg_wall_ms": round(fg_wall_s * 1000.0, 2),
        "fg_p50_ms": round(p50, 3) if p50 is not None else None,
        "fg_p95_ms": round(p95, 3) if p95 is not None else None,
        "fg_p99_ms": round(p99, 3) if p99 is not None else None,
        "fg_max_stall_ms": round(max_stall, 3) if max_stall is not None else None,
        "browser_nav_ms": round(browser_ms, 2) if browser_ms is not None else None,
        "fg_peak_kib": fg_peak // 1024,
        "bg_peak_kib": bg_peak // 1024,
        "bg_high_throttle_events": bg_events.get("high", 0),
        "fg_oom_events": fg_events.get("oom", 0) + fg_events.get("oom_kill", 0),
        "bg_oom_events": bg_events.get("oom", 0) + bg_events.get("oom_kill", 0),
        "pgmajfault_delta": max(0, vm_after.get("pgmajfault", 0) - vm_before.get("pgmajfault", 0)),
        "pswpin_delta": max(0, vm_after.get("pswpin", 0) - vm_before.get("pswpin", 0)),
        "pswpout_delta": max(0, vm_after.get("pswpout", 0) - vm_before.get("pswpout", 0)),
        "mem_psi_some_us_delta": delta_psi(psi_mem_before, psi_mem_after),
        "cpu_psi_some_us_delta": delta_psi(psi_cpu_before, psi_cpu_after),
        "io_psi_some_us_delta": delta_psi(psi_io_before, psi_io_after),
        "daemon_rss_kib": daemon_metrics.get("rss_kib"),
        "daemon_pss_kib": daemon_metrics.get("pss_kib"),
        "daemon_fds": daemon_metrics.get("fd_count"),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cgroup-parent", required=True)
    parser.add_argument("--xynoos-bin", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--trials", type=int, default=5)
    parser.add_argument("--seed", type=int, default=20261125)
    args = parser.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    srv, http_port = start_local_http_server()

    profiles = {
        "profile_a_2g_2cpu_tight": {
            "logical_limit_desc": "2 GiB RAM / 2 CPUs profile with 256 MiB session contention ceiling",
            "session_mem_bytes": 256 * 1024 * 1024,
            "cpu_max": "200000 100000",
            "bg_alloc_mib": 248,
        },
        "profile_b_4g_4cpu_n5000_approx": {
            "logical_limit_desc": "4 GiB RAM / 4 CPUs profile with 384 MiB session contention ceiling",
            "session_mem_bytes": 384 * 1024 * 1024,
            "cpu_max": "400000 100000",
            "bg_alloc_mib": 368,
        },
        "profile_c_8g_4cpu_desktop_ref": {
            "logical_limit_desc": "8 GiB RAM / 4 CPUs reference profile with 768 MiB session ceiling (uncontended)",
            "session_mem_bytes": 768 * 1024 * 1024,
            "cpu_max": "400000 100000",
            "bg_alloc_mib": 220,
        },
    }

    candidates = [
        "baseline_a_std_linux",
        "baseline_b_zram_or_swap",
        "candidate_c_observe_only",
        "candidate_d_advisory",
        "candidate_e_transactional",
    ]
    workloads = ["interactive_probe", "dev_compile", "mixed_multitask"]
    if shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser"):
        workloads.append("browser_headless")

    rng = random.Random(args.seed)
    all_rows = []
    with tempfile.TemporaryDirectory(prefix="xynoos-lab-") as tmp:
        for prof_name, prof_cfg in profiles.items():
            # Focus full 5-trial sweeps on Profile B (4 GiB/4 CPU) and Profile A (2 GiB/2 CPU), 3 trials on unpressured Profile C
            n_trials = args.trials if prof_name != "profile_c_8g_4cpu_desktop_ref" else min(3, args.trials)
            for wl in workloads:
                if prof_name == "profile_c_8g_4cpu_desktop_ref" and wl != "interactive_probe":
                    continue
                for trial in range(n_trials):
                    order = list(candidates)
                    rng.shuffle(order)
                    for cand in order:
                        row = run_single_trial(
                            args.cgroup_parent,
                            args.xynoos_bin,
                            cand,
                            prof_cfg,
                            wl,
                            http_port,
                            tmp,
                        )
                        row["profile"] = prof_name
                        row["trial"] = trial
                        all_rows.append(row)

    srv.shutdown()

    # Write CSV
    csv_path = out_dir / "lab_trials.csv"
    if all_rows:
        with open(csv_path, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=list(all_rows[0].keys()))
            writer.writeheader()
            writer.writerows(all_rows)

    # Aggregate statistics
    summary = {}
    for prof_name in profiles:
        summary[prof_name] = {}
        for wl in workloads:
            sub = [r for r in all_rows if r["profile"] == prof_name and r["workload"] == wl]
            if not sub:
                continue
            summary[prof_name][wl] = {}
            for cand in candidates:
                c_rows = [r for r in sub if r["candidate"] == cand]
                if not c_rows:
                    continue
                summary[prof_name][wl][cand] = {
                    "trials": len(c_rows),
                    "fg_ok_rate": round(sum(1 for r in c_rows if r["fg_ok"]) / len(c_rows), 3),
                    "bg_ok_rate": round(sum(1 for r in c_rows if r["bg_ok"]) / len(c_rows), 3),
                    "total_wall_ms": summarize([r["total_wall_ms"] for r in c_rows]),
                    "fg_wall_ms": summarize([r["fg_wall_ms"] for r in c_rows]),
                    "fg_p50_ms": summarize([r["fg_p50_ms"] for r in c_rows if r["fg_p50_ms"] is not None]),
                    "fg_p95_ms": summarize([r["fg_p95_ms"] for r in c_rows if r["fg_p95_ms"] is not None]),
                    "fg_p99_ms": summarize([r["fg_p99_ms"] for r in c_rows if r["fg_p99_ms"] is not None]),
                    "fg_max_stall_ms": summarize([r["fg_max_stall_ms"] for r in c_rows if r["fg_max_stall_ms"] is not None]),
                    "mem_psi_some_us": summarize([r["mem_psi_some_us_delta"] for r in c_rows if r["mem_psi_some_us_delta"] is not None]),
                    "pgmajfault_delta": summarize([r["pgmajfault_delta"] for r in c_rows]),
                    "bg_high_throttle_events": summarize([r["bg_high_throttle_events"] for r in c_rows]),
                    "daemon_rss_kib": summarize([r["daemon_rss_kib"] for r in c_rows if r["daemon_rss_kib"] is not None]),
                }

    env_meta = {
        "kernel": read_text("/proc/sys/kernel/osrelease"),
        "arch": os.uname().machine,
        "logical_cpus": os.cpu_count(),
        "swaps": read_text("/proc/swaps"),
        "zram_devices": sorted(p.name for p in Path("/sys/block").glob("zram*")),
        "seed": args.seed,
        "browser_available": bool(shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser")),
    }
    report = {"environment": env_meta, "profiles": profiles, "summary": summary}
    (out_dir / "lab_summary.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
