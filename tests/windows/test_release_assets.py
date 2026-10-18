import importlib.util,json,hashlib,tempfile,unittest,zipfile
from pathlib import Path
spec=importlib.util.spec_from_file_location('release_assets',Path(__file__).resolve().parents[2]/'scripts/windows/release_assets.py');gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
class Assets(unittest.TestCase):
 def test_missing_platform_preview_and_tamper_are_rejected(self):
  with tempfile.TemporaryDirectory() as tmp:
   d=Path(tmp);v='1.0.15'
   self.assertTrue(gate.validate(d,v))
   for suffix in ('.zip','.dmg','-windows-x64-setup.exe'):(d/f'BeeK-v{v}{suffix}').write_bytes(b'public fixture')
   payload={'beek.exe':b'public main','beek-selection-host.exe':b'public helper'};manifest={'version':v,'mode':'portable','distribution':'internal-preview-unsigned','files':[{'name':n,'sha256':hashlib.sha256(b).hexdigest()} for n,b in payload.items()]}
   with zipfile.ZipFile(d/f'BeeK-v{v}-windows-x64.zip','w') as z:
    for n,b in payload.items():z.writestr(n,b)
    z.writestr('manifest.json',json.dumps(manifest))
   for f in d.iterdir():f.with_name(f.name+'.sha256').write_text(hashlib.sha256(f.read_bytes()).hexdigest()+'  '+f.name+'\n')
   self.assertEqual(gate.validate(d,v,False),[])
   self.assertTrue(any('internal build' in e for e in gate.validate(d,v,False,True)))
   manifest['distribution']='public-preview-unsigned'
   with zipfile.ZipFile(d/f'BeeK-v{v}-windows-x64.zip','w') as z:
    for n,b in payload.items():z.writestr(n,b)
    z.writestr('manifest.json',json.dumps(manifest))
   f=d/f'BeeK-v{v}-windows-x64.zip';f.with_name(f.name+'.sha256').write_text(hashlib.sha256(f.read_bytes()).hexdigest()+'  '+f.name+'\n')
   self.assertEqual(gate.validate(d,v,False,True),[])
   self.assertTrue(any('unsigned preview' in e for e in gate.validate(d,v)))
   (d/f'BeeK-v{v}.dmg').write_bytes(b'changed');self.assertTrue(any('checksum mismatch' in e for e in gate.validate(d,v,False)))
if __name__=='__main__':unittest.main()
