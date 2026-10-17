import json
import unittest
from pathlib import Path

class MediaRegistry(unittest.TestCase):
 def test_headless_providers_and_media(self):
  entries=json.loads((Path(__file__).resolve().parents[1]/'registry/plugins.json').read_text())['plugins']
  for component in ['youtube','soundcloud','spotify','media','downloads']:
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

 def test_media_bundle_dependencies_are_discoverable(self):
  entries=json.loads((Path(__file__).resolve().parents[1]/'registry/plugins.json').read_text())['plugins']
  by_id={item['id']:item for item in entries}
  media=by_id['dev.delta.media']
  dependencies={item['id']:item for item in media['manifest']['dependencies'] if item['kind']=='plugin'}
  self.assertEqual(set(dependencies),{'dev.delta.'+name for name in ['youtube','soundcloud','spotify','downloads']})
  for plugin_id,dependency in dependencies.items():
   self.assertFalse(dependency['required'])
   self.assertEqual(dependency['version'],'>='+by_id[plugin_id]['version'])

if __name__=='__main__':unittest.main()
