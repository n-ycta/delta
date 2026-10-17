import json
import unittest
from pathlib import Path

class MediaRegistry(unittest.TestCase):
 def test_headless_providers_and_media(self):
  entries=json.loads((Path(__file__).resolve().parents[1]/'registry/plugins.json').read_text())['plugins']
  for component in ['youtube','soundcloud','spotify','media']:
   item=next(p for p in entries if p['component']==component)
   self.assertEqual(item['id'],'dev.delta.'+component)
   self.assertIn('/v'+item['version']+'/',item['asset'])
   self.assertRegex(item['sha256'],r'^[0-9a-f]{64}$')
   self.assertEqual(item['manifest']['runtime']['execution'],'process')
   self.assertNotIn('entry',item['manifest'])
   self.assertNotIn('admin',item['scopes'])
   self.assertNotIn('call:*',item['scopes'])
   self.assertEqual({p['scope'] for p in item['manifest']['permissions']},set(item['scopes']))
   self.assertEqual(item['manifest']['version'],item['version'])

if __name__=='__main__':unittest.main()
