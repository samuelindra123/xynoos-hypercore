#!/usr/bin/env python3
"""Idempotent promotion after external release verification; never replace package bytes."""
import argparse
import os
from pathlib import Path
from archive import verify, publish, require, digest

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path)
parser.add_argument('release', type=Path)
parser.add_argument('--commit', required=True)
parser.add_argument('--version', required=True)
args = parser.parse_args()
fingerprint = os.environ['XYNOOS_APT_FINGERPRINT']
debs = list(args.release.glob('*.deb'))
require(len(debs) == 1, 'Expected exactly one release package')
if args.root.exists() and any(args.root.iterdir()):
    ledger = verify(args.root, fingerprint, allow_expired=True)
    existing = ledger['versions'].get(args.version)
    if existing:
        require(existing['sha256'] == digest(debs[0]) and existing['source_commit'] == args.commit,
                'Existing version differs from attested release; never overwrite')
        publish(args.root, None, None, fingerprint, renew_expired=True)
    else:
        # Expired state must be renewed without adding bytes, then reverified normally.
        publish(args.root, None, None, fingerprint, renew_expired=True)
        publish(args.root, debs[0], args.commit, fingerprint)
else:
    publish(args.root, debs[0], args.commit, fingerprint)
