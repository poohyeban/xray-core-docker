#!/usr/bin/env python3
"""Prepare runtime directories and geodata; never create or modify Xray config."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def run(args, capture=False):
    return subprocess.run(args, cwd=ROOT, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None,
                          stderr=subprocess.PIPE if capture else None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-pull', action='store_true', help='Use already available images (local builds / CI)')
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error('Run as root on the target Linux VPS')
    os.umask(0o027)
    model = json.loads(run(['docker', 'compose', 'config', '--format', 'json'], capture=True).stdout)
    image = model['services']['geodata-updater']['image']
    if not args.skip_pull:
        run(['docker', 'compose', 'pull'])
    for directory in ['xray/config', 'xray/certs', 'xray/geodata', 'xray/log', 'xray-geodata-updater/log']:
        (ROOT / directory).mkdir(parents=True, mode=0o750, exist_ok=True)
    geodata = ROOT / 'xray/geodata'
    present = [(geodata / name).is_file() for name in ['geoip.dat', 'geosite.dat']]
    if any(present) and not all(present):
        raise RuntimeError('Incomplete geodata; inspect the directory before retrying')
    if not any(present):
        run(['docker', 'run', '--rm', '--read-only', '--cap-drop=ALL',
             '--security-opt=no-new-privileges:true', '--tmpfs', '/tmp',
             '-v', f'{geodata}:/geodata', '--entrypoint', '/usr/local/bin/bootstrap-geodata.sh', image])
    print('Runtime directories and geodata are ready. No Xray configuration was created or modified.')
    if not (ROOT / 'xray/config/config.json').is_file():
        print('REQUIRED: provide your own xray/config/config.json before starting Xray.')
    print('The stack cannot start successfully without a valid Xray configuration.')


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError:
        sys.exit('Preparation command failed; inspect the command status without publishing private configuration.')
    except (RuntimeError, OSError, ValueError) as error:
        sys.exit(str(error))
