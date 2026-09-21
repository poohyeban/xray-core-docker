#!/usr/bin/env python3
"""Verify preparation cannot generate or overwrite a deployment identity."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('prepare', Path(__file__).with_name('prepare.py'))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / 'xray/geodata').mkdir(parents=True)
        for name in ['geoip.dat', 'geosite.dat']:
            (self.root / 'xray/geodata' / name).write_bytes(b'test-only data')
        self.old_umask = os.umask(0o027)

    def tearDown(self):
        os.umask(self.old_umask)
        self.temp.cleanup()

    def invoke(self):
        output = io.StringIO()
        model = {'services': {'geodata-updater': {'image': 'updater:test'}}}
        with patch.object(prepare, 'ROOT', self.root), patch.object(prepare.os, 'geteuid', return_value=0), \
             patch.object(prepare, 'run', return_value=SimpleNamespace(stdout=json.dumps(model))), \
             patch('sys.argv', ['prepare.py', '--skip-pull']), contextlib.redirect_stdout(output):
            prepare.main()
        return output.getvalue()

    def test_missing_config_is_not_generated(self):
        output = self.invoke()
        self.assertFalse((self.root / 'xray/config/config.json').exists())
        self.assertIn('REQUIRED', output)

    def test_existing_config_is_untouched(self):
        directory = self.root / 'xray/config'
        directory.mkdir()
        config = directory / 'config.json'
        content = b'{"test": "do not modify"}\n'
        config.write_bytes(content)
        self.invoke()
        self.assertEqual(content, config.read_bytes())

    def test_partial_geodata_is_not_silently_overwritten(self):
        (self.root / 'xray/geodata/geosite.dat').unlink()
        with self.assertRaisesRegex(RuntimeError, 'Incomplete geodata'):
            self.invoke()
        self.assertEqual((self.root / 'xray/geodata/geoip.dat').read_bytes(), b'test-only data')


if __name__ == '__main__':
    unittest.main()
