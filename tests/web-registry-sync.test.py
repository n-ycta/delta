import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import unittest

spec = importlib.util.spec_from_file_location('sync', Path(__file__).resolve().parents[1] / 'scripts/sync-web-registry.py')
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)


class RegistrySyncTests(unittest.TestCase):
    def setUp(self):
        self.catalog = {'plugins': [{'id': 'dev.delta.web', 'version': '0.6.11', 'scopes': ['admin']},
                                   {'id': 'other', 'version': '1.0.0'}]}
        self.release = {'tag_name': 'v0.6.13', 'draft': False, 'prerelease': False}
        self.manifest = {'id': 'web', 'version': '0.6.13', 'metadata': {'ui': {
            'id': 'dev.delta.web', 'version': '0.6.13', 'dependencies': []}}}

    def package(self):
        data = json.dumps(self.manifest).encode()
        out = io.BytesIO()
        with tarfile.open(fileobj=out, mode='w:gz') as tar:
            member = tarfile.TarInfo('manifest.json')
            member.size = len(data)
            tar.addfile(member, io.BytesIO(data))
        archive = out.getvalue()
        return archive, hashlib.sha256(archive).hexdigest() + '  delta-web.tar.gz\n'

    def test_verified_update_and_idempotence(self):
        archive, sums = self.package()
        other = copy.deepcopy(self.catalog['plugins'][1])
        self.assertTrue(sync.update(self.catalog, self.release, archive, sums))
        self.assertEqual(self.catalog['plugins'][0]['version'], '0.6.13')
        self.assertEqual(self.catalog['plugins'][0]['scopes'], ['admin'])
        self.assertEqual(self.catalog['plugins'][1], other)
        self.assertFalse(sync.update(self.catalog, self.release, archive, sums))

    def test_bad_checksum_leaves_catalog_untouched(self):
        before = copy.deepcopy(self.catalog)
        with self.assertRaises(ValueError):
            sync.update(self.catalog, self.release, self.package()[0], '0' * 64 + '  delta-web.tar.gz')
        self.assertEqual(before, self.catalog)

    def test_wrong_identity(self):
        self.manifest['metadata']['ui']['id'] = 'dev.other'
        with self.assertRaises(ValueError):
            sync.update(self.catalog, self.release, *self.package())

    def test_downgrade_and_prerelease_rejected(self):
        for changes in ({'tag_name': 'v0.6.10'}, {'prerelease': True}, {'tag_name': 'v0.7.0-beta'}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                sync.update(self.catalog, {**self.release, **changes}, *self.package())

    def test_same_version_replacement_rejected(self):
        sync.update(self.catalog, self.release, *self.package())
        self.manifest['metadata']['ui']['dependencies'] = [{'id': 'different'}]
        with self.assertRaises(ValueError):
            sync.update(self.catalog, self.release, *self.package())


if __name__ == '__main__':
    unittest.main()
