import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('portable_zip', ROOT / 'scripts/windows/portable_zip.py')
portable_zip = importlib.util.module_from_spec(spec)
spec.loader.exec_module(portable_zip)


class PortableZipTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.payload = self.root / 'payload'
        self.payload.mkdir()
        self.output = self.root / 'beek.zip'
        self.files = {'beek.exe': b'public-main-fixture', 'beek-selection-host.exe': b'public-helper-fixture', 'LICENSE': b'public license fixture'}
        for name, body in self.files.items():
            (self.payload / name).write_bytes(body)
        self.manifest = {'files': [{'name': name, 'bytes': len(body), 'sha256': hashlib.sha256(body).hexdigest()} for name, body in self.files.items()]}
        self.save_manifest()

    def tearDown(self):
        self.temp.cleanup()

    def save_manifest(self):
        (self.payload / 'manifest.json').write_text(json.dumps(self.manifest))

    def test_archive_contains_only_verified_flat_inventory(self):
        self.assertEqual(portable_zip.build(self.payload, self.output), 4)
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(set(archive.namelist()), set(self.files) | {'manifest.json'})
            for name, body in self.files.items():
                self.assertEqual(archive.read(name), body)

    def test_changed_payload_cannot_produce_a_valid_artifact(self):
        (self.payload / 'beek.exe').write_bytes(b'X' + self.files['beek.exe'][1:])
        with self.assertRaises(ValueError):
            portable_zip.build(self.payload, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('*.tmp')), [])

    def test_unlisted_files_and_path_escape_are_rejected(self):
        extra = self.payload / 'unlisted.json'
        extra.write_text('public extra fixture')
        with self.assertRaises(ValueError):
            portable_zip.build(self.payload, self.output)
        extra.unlink()
        self.manifest['files'][0]['name'] = '../outside.exe'
        self.save_manifest()
        with self.assertRaises(ValueError):
            portable_zip.build(self.payload, self.output)
        self.assertFalse(self.output.exists())

    def test_case_collisions_and_existing_outputs_are_preserved(self):
        self.manifest['files'].append(dict(self.manifest['files'][0], name='BEEK.EXE'))
        self.save_manifest()
        with self.assertRaises(ValueError):
            portable_zip.build(self.payload, self.output)
        self.output.write_bytes(b'existing artifact')
        with self.assertRaises(ValueError):
            portable_zip.build(self.payload, self.output)
        self.assertEqual(self.output.read_bytes(), b'existing artifact')
