#!/usr/bin/env python3
"""Small userspace comparison; never launches services or changes kernel controls."""
import argparse
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
MODES = ("baseline", "observe", "simulate")


def read(path):
    try:
        with open(path, "rb") as file:
            return file.read(1048577).decode()[:1048576]
    except (OSError, UnicodeError):
        return None


def context():
    vm = {}
    for line in (read("/proc/vmstat") or "").splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[0] in ("pswpin", "pswpout"):
            vm[fields[0]] = int(fields[1])
    psi = {}
    for line in (read("/proc/pressure/memory") or "").splitlines():
        name, *fields = line.split()
        psi[name] = {key: float(value) if key != "total" else int(value)
                     for key, value in (field.split("=", 1) for field in fields)}
    return {"memory_psi": psi or None, "swap_page_counters": vm or None,
            "meminfo": read("/proc/meminfo")}


def run(command):
    before = context()
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        started = time.perf_counter_ns()
        proc = subprocess.Popen(command, stdout=stdout, stderr=stderr, cwd=ROOT)
        deadline = time.monotonic() + 15
        while True:
            pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                break
            if time.monotonic() > deadline:
                proc.kill()
                _, status, usage = os.wait4(proc.pid, 0)
                proc.returncode = os.waitstatus_to_exitcode(status)
                raise RuntimeError("owned benchmark process timed out and was reaped")
            time.sleep(0.001)
        process_wall_ns = time.perf_counter_ns() - started
        proc.returncode = os.waitstatus_to_exitcode(status)
        stdout.seek(0)
        stderr.seek(0)
        if proc.returncode:
            raise RuntimeError(stderr.read(65536).decode(errors="replace"))
        output = stdout.read(1048577)
        if len(output) > 1048576:
            raise RuntimeError("child report exceeds 1 MiB")
    return {"command": list(map(str, command)), "process_wall_ns": process_wall_ns,
            "process_user_cpu_ns": round(usage.ru_utime * 1e9),
            "process_system_cpu_ns": round(usage.ru_stime * 1e9),
            "spawn_inclusive_peak_rss_kib": usage.ru_maxrss,
            "host_before": before, "host_after": context(),
            "report": json.loads(output)}


def quantiles(values):
    values = sorted(values)
    if not values:
        return None
    return {"count": len(values), **{
        f"p{percent}": values[max(0, math.ceil(len(values) * percent / 100) - 1)]
        for percent in (50, 95, 99)}}


def summarize(runs):
    result = {}
    for mode in MODES:
        group = [run for run in runs if run["mode"] == mode]
        rows = [row for run in group for row in run["report"]["rows"]]
        result[mode] = {
            "process_wall_ns": quantiles([run["process_wall_ns"] for run in group]),
            "engine_workload_total_ns": quantiles([run["report"]["total_ns"] for run in group]),
            "process_cpu_ns": quantiles([run["process_user_cpu_ns"] + run["process_system_cpu_ns"] for run in group]),
            "process_high_water_rss_kib": quantiles([run["report"]["process_high_water_rss_kib"] for run in group if run["report"]["process_high_water_rss_kib"] is not None]),
            "sampled_loaded_rss_kib": quantiles([row["workload"]["sampled_process_rss_kib"] for row in rows if row["workload"]["sampled_process_rss_kib"] is not None]),
            "decision_ns": quantiles([row["decision_ns"] for row in rows]),
            "response_ns": quantiles([row["response_ns"] for row in rows]),
            "workload_ns": quantiles([row["workload"]["workload_ns"] for row in rows]),
            "stored_payload_bytes": quantiles([row["workload"]["stored_payload_bytes"] for row in rows]),
            "safety_refusals": sum(run["report"]["safety_refusals"] for run in group),
            "private_buffer_verifications": sum(run["report"]["owned_buffer_verifications"] for run in group),
            "compression_steps": sum(row["workload"]["compressed_pages"] > 0 for row in rows),
        }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=12)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 6 <= args.repeats <= 60 or args.repeats % 6:
        parser.error("repeats must be a multiple of 6 within 6..60")
    output = args.output.resolve()
    summary_path = output.with_suffix(".summary.json")
    if output.exists() or summary_path.exists():
        parser.error("refusing to replace existing evidence")
    binary = ROOT / "target/release/xynoos-bench"
    cli = ROOT / "target/release/xynoos-cli"
    # Warm each arm once, discard these measurements, then balance all permutations.
    for mode in MODES:
        run([binary, "--mvp", mode])
    run([cli, "mvp", "observe"])
    runs = []
    orders = list(itertools.permutations(MODES))
    for repeat in range(args.repeats):
        for mode in orders[repeat % len(orders)]:
            measured = run([binary, "--mvp", mode])
            measured.update(mode=mode, repeat=repeat)
            report = measured["report"]
            if report["kernel_mutations"] or not all(row["workload"]["byte_verification_passed"] for row in report["rows"]):
                raise RuntimeError("unexpected mutation or failed workload integrity")
            runs.append(measured)
    if len({(r["report"]["trace_crc32"], r["report"]["workload_crc32"]) for r in runs}) != 1:
        raise RuntimeError("comparison trace/workload mismatch")
    live = [run([cli, "mvp", "observe"]) for _ in range(args.repeats)]
    summary = {"schema_version": 1,
               "evidence": "Codespaces userspace measurements; pressure replay is simulation; no physical A/B",
               "repeats_per_arm": args.repeats, "arms": summarize(runs),
               "live_observe_separate": {
                   "collection_ns": quantiles([r["report"]["telemetry"]["collection_duration_ns"] for r in live]),
                   "decision_ns": quantiles([r["report"]["decision_ns"] for r in live]),
                   "process_cpu_ns": quantiles([r["process_user_cpu_ns"] + r["process_system_cpu_ns"] for r in live]),
               }}
    evidence = {"schema_version": 1, "summary": summary,
                "environment": {"platform": platform.platform(), "machine": platform.machine(),
                    "cpu_model": next((line.split(":",1)[1].strip() for line in (read("/proc/cpuinfo") or "").splitlines() if line.startswith("model name")), None),
                    "source_base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    "binary_sha256": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in (binary,cli)},
                    "trace_sha256": hashlib.sha256((ROOT / "fixtures/mvp-trace.json").read_bytes()).hexdigest()},
                "runs": runs, "live_observe": live}
    source_paths = sorted([p for p in (ROOT / "crates").rglob("*") if p.suffix in (".rs", ".toml")] + [ROOT / "Cargo.toml", ROOT / "Cargo.lock"])
    evidence["environment"]["source_code_sha256"] = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths}
    with output.open("x") as file:
        json.dump(evidence, file, separators=(",", ":")); file.write("\n")
    with summary_path.open("x") as file:
        json.dump(summary, file, indent=2); file.write("\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
