#!/usr/bin/env python3
"""Audit published image metadata and runtime-data absence without printing contents."""
import json
import os
from pathlib import Path
import subprocess
import tarfile
import sys
from privacy_check import issues


def check_image(image):
    metadata = subprocess.check_output(['docker', 'image', 'inspect', image], text=True)
    if issues(metadata):
        raise RuntimeError('Image metadata contains a potential private value')
    container = subprocess.check_output(['docker', 'create', image], text=True).strip()
    try:
        process = subprocess.Popen(['docker', 'export', container], stdout=subprocess.PIPE)
        forbidden = []
        with tarfile.open(fileobj=process.stdout, mode='r|') as archive:
            for entry in archive:
                name = entry.name.lstrip('./')
                if entry.isfile() and entry.size and (
                    name.startswith(('geodata/', 'var/log/xray/', 'var/log/xray-geodata-updater/',
                                     'usr/local/etc/xray/', 'etc/xray/certs/', 'root/.ssh/'))
                    or name.endswith(('.pem', '.key')) and not name.startswith(('etc/ssl/', 'usr/share/'))
                ):
                    forbidden.append(name)
        process.stdout.close()
        if process.wait() != 0 or forbidden:
            raise RuntimeError('Image filesystem contains unexpected runtime or key files')
    finally:
        subprocess.run(['docker', 'rm', '-f', container], check=True, stdout=subprocess.DEVNULL)
    print('Image metadata and runtime-data checks passed: ' + image)


def main():
    history = subprocess.check_output(['git', 'log', '--all', '--format=%an <%ae>%n%cn <%ce>%n%B', '-p'], text=True)
    if issues(history):
        raise RuntimeError('Git history contains a potential private value')
    for image in sys.argv[1:]:
        check_image(image)
    print('Full available Git history privacy check passed.')


if __name__ == '__main__':
    main()
