#!/usr/bin/env python3
"""Disposable-container check: standalone manager refuses links to APT-owned files."""
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch

assert Path('/.dockerenv').exists(), 'Disposable container only'
assert os.getuid() != 0, 'Run manager collision checks unprivileged'
assert subprocess.check_output(['dpkg-query', '-W', '-f=${Status}', 'xynoos-hypercore']).decode() == 'install ok installed'
package = Path('/usr/bin/xynoos')
before = hashlib.sha256(package.read_bytes()).hexdigest()
spec = importlib.util.spec_from_file_location('xynoos_standalone', '/work/distribution/manager.py')
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)
with tempfile.TemporaryDirectory(prefix='xynoos-manager-collision-') as directory:
    home = Path(directory)
    with patch.dict(os.environ, HOME=directory, XDG_DATA_HOME=str(home / 'data'), XDG_STATE_HOME=str(home / 'state')):
        installation = manager.Installation()
        installation.bin_dir.mkdir(parents=True, mode=0o700)
        installation.entry.symlink_to(package)
        with patch.object(manager, 'discover', side_effect=AssertionError('Network must not be reached')):
            for operation in [lambda: installation.install('preview'), installation.uninstall]:
                try:
                    with installation.locked():
                        operation()
                except manager.Refusal as error:
                    assert 'belongs to another installation' in str(error), error
                else:
                    raise AssertionError('Standalone manager accepted APT file collision')
        assert installation.entry.is_symlink() and installation.entry.resolve() == package
assert hashlib.sha256(package.read_bytes()).hexdigest() == before
assert subprocess.check_output(['dpkg', '--verify', 'xynoos-hypercore']).strip() == b''
print('PASS: standalone install/uninstall refuses foreign APT symlink before networking; APT payload unchanged')
