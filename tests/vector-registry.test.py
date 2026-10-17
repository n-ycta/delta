"""Regression coverage for the independently installable Vector service."""
import json
import unittest
from pathlib import Path


class VectorRegistry(unittest.TestCase):
    def test_vector_is_installable_and_storage_scoped(self):
        entries = json.loads((Path(__file__).resolve().parents[1] / 'registry/plugins.json').read_text())['plugins']
        vector = next(p for p in entries if p['id'] == 'dev.delta.vector')
        self.assertEqual(vector['component'], 'vector')
        self.assertEqual(vector['manifest']['category'], 'productivity')
        self.assertEqual(vector['manifest']['runtime']['execution'], 'process')
        self.assertEqual(vector['manifest']['version'], vector['version'])
        self.assertEqual(vector['manifest']['id'], vector['id'])
        self.assertEqual(vector['asset'], f"https://github.com/n-ycta/delta-vector/releases/download/v{vector['version']}/delta-vector.tar.gz")
        self.assertRegex(vector['sha256'], r'^[0-9a-f]{64}$')
        self.assertEqual(set(vector['scopes']), {'register:vector', 'call:storage.document_get', 'call:storage.document_put'})
        self.assertEqual({p['scope'] for p in vector['manifest']['permissions']}, set(vector['scopes']))
        dependencies = {d['id']: d for d in vector['manifest']['dependencies']}
        self.assertTrue(dependencies['dev.delta.db']['required'])
        self.assertEqual(dependencies['dev.delta.db']['kind'], 'plugin')
        self.assertIn('dev.delta.db', {p['id'] for p in entries})
        self.assertTrue(dependencies['core']['required'])


if __name__ == '__main__':
    unittest.main()
