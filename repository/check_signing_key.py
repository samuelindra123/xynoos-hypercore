#!/usr/bin/env python3
"""Fail closed unless GNUPGHOME has only the pinned delegated signing subkey."""
import argparse
import os
import re
import subprocess
import time


def require(value, message):
    if not value:
        raise SystemExit(message)


def inspect(fingerprint):
    require(re.fullmatch(r"[A-F0-9]{40}", fingerprint) is not None,
            "Expected full uppercase OpenPGP primary fingerprint")
    result = subprocess.run(
        ["gpg", "--batch", "--fixed-list-mode", "--with-colons", "--list-secret-keys"],
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    records = [line.split(":") for line in result.stdout.splitlines()]
    primaries = [record for record in records if record[0] in {"sec", "sec#", "sec>"}]
    require(len(primaries) == 1, "Signing keyring must contain exactly one primary key")
    primary = primaries[0]
    primary_index = records.index(primary)
    primary_fingerprint = next(
        (record[9] for record in records[primary_index + 1:]
         if record[0] == "fpr"), None
    )
    require(primary_fingerprint == fingerprint, "Signing key primary fingerprint mismatch")
    # GnuPG colon field 15 is '#' for an unavailable secret-key stub. This
    # requires the owner to export subkeys only, leaving the primary offline.
    require(len(primary) > 14 and primary[14] == "#",
            "Primary private key is present; import only the delegated signing subkey")
    now = int(time.time())
    primary_expiry = int(primary[6]) if primary[6] else 0
    require(primary_expiry > now, "Primary public key is expired or has no explicit expiry")
    secret_subkeys = [record for record in records if record[0] == "ssb"
                      and len(record) > 14 and record[14] == "+"]
    require(len(secret_subkeys) == 1, "Import exactly one secret subkey")
    signing = secret_subkeys[0]
    require(len(signing) > 11 and "s" in signing[11],
            "Imported subkey does not have signing capability")
    expiry = int(signing[6]) if signing[6] else 0
    require(expiry > now, "Signing subkey is expired or has no explicit expiry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fingerprint", default=os.environ.get("XYNOOS_APT_FINGERPRINT", ""))
    args = parser.parse_args()
    inspect(args.fingerprint)
    print("PASS: one pinned, expiring signing subkey; primary private key remains offline")


if __name__ == "__main__":
    main()
