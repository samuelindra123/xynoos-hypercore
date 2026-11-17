#!/usr/bin/env python3
"""Check maintained Wiki navigation, required pages, and local Markdown targets."""
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
wiki = root / 'docs/wiki'
expected = {'Home','Getting-Started','Installation','Native-APT-Installation','Direct-Debian-Installation',
            'MX-Linux-Compatibility','Quick-Start','CLI-Reference','Architecture-Overview','HyperMemory','PressureIQ',
            'AURA-Resource-Intelligence','CPU-and-GPU-Hardware-Support','System-Safety-Model','Package-Distribution',
            'Releases-and-Updates','Upgrade-and-Uninstall','Troubleshooting','Development-Setup','Contributing',
            'Performance-Validation','Security-Policy','Reporting-Issues','Roadmap','Known-Limitations','_Sidebar','_Footer'}
assert {p.stem for p in wiki.glob('*.md')} == expected
for path in [root / 'README.md', *wiki.glob('*.md'), *(root / 'docs/distribution').glob('*.md')]:
    content = path.read_text()
    for link in re.findall(r'\]\(([^)]+)\)', content):
        target = link.split('#')[0]
        if target and not re.match(r'[a-z]+:', target):
            assert (path.parent / target).exists(), (path, target)
    for link in re.findall(r'\[\[(.*?)\]\]', content):
        target = link.split('|')[-1].replace(' ', '-')
        assert target in expected, (path, target)
print('25 Wiki pages, sidebar/footer, navigation and local documentation targets verified')
