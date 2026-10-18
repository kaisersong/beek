#!/usr/bin/env python3
"""Fail closed before publishing a stable, complete two-platform release."""
import argparse,hashlib,json,re,zipfile
from pathlib import Path

def validate(folder:Path,version:str,formal=True,public_preview=False):
 errors=[]
 if not re.fullmatch(r'\d+\.\d+\.\d+',version):return ['invalid release version']
 names=[f'BeeK-v{version}{suffix}' for suffix in ('.zip','.dmg','-windows-x64.zip','-windows-x64-setup.exe')]
 for name in names:
  file=folder/name;checksum=folder/(name+'.sha256')
  if not file.is_file() or not checksum.is_file():errors.append('missing required asset/checksum: '+name);continue
  expected=hashlib.sha256(file.read_bytes()).hexdigest()
  if checksum.read_text().strip()!=expected+'  '+name:errors.append('asset checksum mismatch: '+name)
 winzip=folder/names[2]
 try:
  with zipfile.ZipFile(winzip) as archive:
   members=archive.namelist()
   if any(n.startswith(('/','\\')) or '..' in Path(n.replace('\\','/')).parts for n in members):errors.append('unsafe archive path')
   manifest=json.loads(archive.read('manifest.json'))
   if manifest.get('version')!=version or manifest.get('mode')!='portable':errors.append('wrong Windows manifest version/mode')
   if public_preview and manifest.get('distribution') not in ('formal-signed','public-preview-unsigned'):errors.append('internal build is not a public Windows preview')
   if formal and manifest.get('distribution')!='formal-signed':errors.append('unsigned preview cannot be published')
   listed={entry['name'] for entry in manifest['files']}
   if not {'beek.exe','beek-selection-host.exe'}.issubset(listed):errors.append('incomplete Windows executable pair')
   if set(members)!=listed|{'manifest.json'}:errors.append('unlisted or missing payload files')
   for entry in manifest['files']:
    if '/' in entry['name'] or '\\' in entry['name']:errors.append('nested payload path');continue
    if hashlib.sha256(archive.read(entry['name'])).hexdigest()!=entry['sha256']:errors.append('payload hash mismatch: '+entry['name'])
 except (OSError,ValueError,KeyError,zipfile.BadZipFile):errors.append('invalid Windows package manifest')
 if formal:
  try:
   report=json.loads((folder/'windows-signatures.json').read_text())
   for name in ('beek.exe','beek-selection-host.exe',names[3]):
    item=report['files'][name]
    if item['status']!='Valid' or item['thumbprint']!=report['expected_thumbprint']:errors.append('invalid Windows signature: '+name)
    if name==names[3]:actual=hashlib.sha256((folder/name).read_bytes()).hexdigest()
    else:
     with zipfile.ZipFile(winzip) as archive:actual=hashlib.sha256(archive.read(name)).hexdigest()
    if item['sha256']!=actual:errors.append('signature attestation asset mismatch: '+name)
   if not report.get('signed_uninstaller'):errors.append('unsigned uninstaller')
  except (OSError,ValueError,KeyError,zipfile.BadZipFile):errors.append('missing or invalid Windows signature attestation')
 return errors

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--directory',type=Path,required=True);p.add_argument('--version',required=True);mode=p.add_mutually_exclusive_group();mode.add_argument('--internal-preview',action='store_true');mode.add_argument('--allow-unsigned-windows',action='store_true');a=p.parse_args();errors=validate(a.directory,a.version,not (a.internal_preview or a.allow_unsigned_windows),a.allow_unsigned_windows)
 for error in errors:print('NOT PASS: '+error)
 return 1 if errors else 0
if __name__=='__main__':raise SystemExit(main())
