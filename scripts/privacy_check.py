#!/usr/bin/env python3
"""Fail closed on runtime files and obvious credentials; never print matched values."""
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
ALLOWED_ROOT = {'.gitignore', '.dockerignore', '.env.example', 'README.md', 'docker-compose.yml'}
ALLOWED = ALLOWED_ROOT | {
    '.github/workflows/build.yml',
    'scripts/prepare.py', 'scripts/privacy_check.py', 'scripts/test_prepare.py',
    'xray-controller/.dockerignore', 'xray-controller/Dockerfile',
    'xray-controller/Cargo.toml', 'xray-controller/Cargo.lock', 'xray-controller/src/main.rs',
    'xray-geodata-updater/.dockerignore', 'xray-geodata-updater/Dockerfile',
    'xray-geodata-updater/entrypoint.sh', 'xray-geodata-updater/healthcheck.sh',
    'xray-geodata-updater/update-geodata.sh', 'xray-geodata-updater/bootstrap-geodata.sh',
    'xray-geodata-updater/root.cron', 'xray-geodata-updater/logrotate.conf',
}
PATTERNS = {
    'private key': r'-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----',
    'GitHub credential': r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b',
    'UUID': r'\b[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}\b',
    'local home path': r'/Users/[A-Za-z0-9_.-]+/',
}


def issues(text):
    found = [kind for kind, pattern in PATTERNS.items() if re.search(pattern, text)]
    for address in re.findall(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}', text):
        domain = address.rsplit('@', 1)[1]
        if domain not in {'users.noreply.github.com', 'github.com', 'example.com', 'example.invalid'}:
            found.append('personal email')
    return sorted(set(found))


def main():
    paths = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    failures = []
    for name in filter(None, paths):
        path = ROOT / name
        if name not in ALLOWED or path.is_symlink():
            failures.append((name, ['not in publish allowlist']))
            continue
        found = issues(path.read_text())
        if found:
            failures.append((name, found))
    if failures:
        for name, kinds in failures:
            print(name + ': ' + ', '.join(kinds), file=sys.stderr)
        return 1
    print('Privacy allowlist and credential checks passed.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
