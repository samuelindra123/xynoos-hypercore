#!/usr/bin/env python3
"""Independent reference C liblz4 interoperability & deterministic fuzz test for HyperCore LZ4/LZ4HC."""
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def load_liblz4():
    for candidate in ("liblz4.so.1", ctypes.util.find_library("lz4")):
        if not candidate:
            continue
        try:
            lib = ctypes.CDLL(candidate)
            lib.LZ4_compress_default.argtypes = [
                ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int
            ]
            lib.LZ4_compress_default.restype = ctypes.c_int
            lib.LZ4_decompress_safe.argtypes = [
                ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int
            ]
            lib.LZ4_decompress_safe.restype = ctypes.c_int
            lib.LZ4_compressBound.argtypes = [ctypes.c_int]
            lib.LZ4_compressBound.restype = ctypes.c_int
            has_hc = hasattr(lib, "LZ4_compress_HC")
            if has_hc:
                lib.LZ4_compress_HC.argtypes = [
                    ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int, ctypes.c_int
                ]
                lib.LZ4_compress_HC.restype = ctypes.c_int
            return lib, candidate, has_hc
        except OSError:
            continue
    return None, None, False


def xorshift64(seed):
    x = seed & 0xFFFFFFFFFFFFFFFF
    while True:
        x ^= (x << 13) & 0xFFFFFFFFFFFFFFFF
        x ^= (x >> 7) & 0xFFFFFFFFFFFFFFFF
        x ^= (x << 17) & 0xFFFFFFFFFFFFFFFF
        yield x


def generate_vectors(seed=0x4D49_4C45_5354_4F4E):
    rng = xorshift64(seed)
    vectors = []
    # Fixed boundary vectors
    vectors.append(("empty", b""))
    for sz in (1, 2, 4, 11, 12, 13, 15, 16, 64, 512, 4096, 16384, 65536, 262144, 1048576):
        pat = b"XYNOOS-HYPERCORE-INTEROP-0123456789ABCDEF-"
        vectors.append((f"rep_{sz}", (pat * ((sz // len(pat)) + 1))[:sz]))

    # Offset boundary ~65535
    buf = bytearray(os.urandom(0))
    b = bytearray(70000)
    for i in range(len(b)):
        b[i] = next(rng) & 0xFF
    anchor = b"OFFSET_BOUNDARY_MATCH_ANCHOR_65535"
    b[0:len(anchor)] = anchor
    b[65535:65535 + len(anchor)] = anchor
    vectors.append(("offset_65535", bytes(b)))

    # 150 deterministic seeded mixed vectors
    for idx in range(150):
        sz = next(rng) % 32768
        kind = idx % 4
        if kind == 0:
            data = bytes([0] * sz)
        elif kind == 1:
            p = f'{{"id":{idx},"status":"ok","payload":"xynoos"}}\n'.encode()
            data = (p * ((sz // len(p)) + 1))[:sz]
        elif kind == 2:
            raw = bytearray(sz)
            for j in range(sz):
                raw[j] = (next(rng) & 0xFF) if (j % 8 >= 5) else ((j // 8) & 0x1F)
            data = bytes(raw)
        else:
            raw = bytearray(sz)
            for j in range(sz):
                raw[j] = next(rng) & 0xFF
            data = bytes(raw)
        vectors.append((f"seed_{idx}_{sz}", data))
    return vectors


def run_bench_codec(bench_bin, op, arg, payload):
    proc = subprocess.run(
        [bench_bin, "codec-op", op, str(arg)],
        input=payload,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"xynoos-bench codec-op {op} {arg} failed: {proc.stderr.decode()}")
    return proc.stdout


def main():
    if len(sys.argv) < 2:
        print("usage: lz4_interop_test.py <path-to-xynoos-bench>", file=sys.stderr)
        sys.exit(2)
    bench_bin = sys.argv[1]
    lib, lib_name, has_hc = load_liblz4()
    if lib is None:
        print(json.dumps({"status": "BLOCKED", "reason": "liblz4.so.1 not installed"}))
        sys.exit(1)

    seed = 0x4D49_4C45_5354_4F4E
    vectors = generate_vectors(seed)
    t0 = time.monotonic()
    hc_to_ref_ok = 0
    ref_std_to_hc_ok = 0
    ref_hc_to_hc_ok = 0

    for name, data in vectors:
        # 1. HyperCore LZ4HC (level 6 & 9) -> Reference liblz4 LZ4_decompress_safe
        for level in (6, 9):
            compressed = run_bench_codec(bench_bin, "encode-hc", level, data)
            out_cap = max(1, len(data))
            out_buf = ctypes.create_string_buffer(out_cap)
            dec_len = lib.LZ4_decompress_safe(
                compressed, out_buf, len(compressed), len(data)
            )
            if dec_len != len(data) or out_buf.raw[:dec_len] != data:
                raise AssertionError(
                    f"HyperCore->liblz4 mismatch on {name} level={level}: dec_len={dec_len} expected={len(data)}"
                )
            hc_to_ref_ok += 1

        # 2. Reference liblz4 LZ4_compress_default -> HyperCore decompress_with_codec
        bound = lib.LZ4_compressBound(len(data))
        c_buf = ctypes.create_string_buffer(max(1, bound))
        c_len = lib.LZ4_compress_default(data, c_buf, len(data), max(1, bound))
        if c_len <= 0:
            raise AssertionError(f"liblz4 compress_default failed on {name}")
        restored = run_bench_codec(bench_bin, "decode-lz4", len(data), c_buf.raw[:c_len])
        if restored != data:
            raise AssertionError(f"liblz4_default->HyperCore mismatch on {name}")
        ref_std_to_hc_ok += 1

        # 3. Reference liblz4 LZ4_compress_HC -> HyperCore decompress_with_codec
        if has_hc:
            hc_buf = ctypes.create_string_buffer(max(1, bound))
            hc_len = lib.LZ4_compress_HC(data, hc_buf, len(data), max(1, bound), 9)
            if hc_len <= 0:
                raise AssertionError(f"liblz4 compress_HC failed on {name}")
            restored_hc = run_bench_codec(
                bench_bin, "decode-lz4", len(data), hc_buf.raw[:hc_len]
            )
            if restored_hc != data:
                raise AssertionError(f"liblz4_HC->HyperCore mismatch on {name}")
            ref_hc_to_hc_ok += 1

    elapsed_ms = round((time.monotonic() - t0) * 1000.0, 2)
    report = {
        "status": "PASS",
        "reference_library": lib_name,
        "has_liblz4_hc": has_hc,
        "seed": hex(seed),
        "vector_count": len(vectors),
        "hypercore_hc_to_liblz4_verified": hc_to_ref_ok,
        "liblz4_default_to_hypercore_verified": ref_std_to_hc_ok,
        "liblz4_hc_to_hypercore_verified": ref_hc_to_hc_ok,
        "elapsed_ms": elapsed_ms,
        "failures": 0,
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
