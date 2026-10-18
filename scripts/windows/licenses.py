#!/usr/bin/env python3
"""Collect license declarations and shipped license texts from the locked crates."""
import argparse,json,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--strict',action='store_true');p.add_argument('--missing-json',type=Path);args=p.parse_args()
root=Path(__file__).resolve().parents[2]
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--filter-platform','x86_64-pc-windows-msvc'],cwd=root))
nodes={n['id']:n for n in metadata['resolve']['nodes']};resolved=set();pending=[metadata['resolve']['root']]
while pending:
 node=pending.pop()
 if node in resolved:continue
 resolved.add(node);pending.extend(d['pkg'] for d in nodes[node]['deps'])
lines=['BeeK locked Windows dependency notices','Generated from Cargo.lock and the installed crate sources.',''];missing=[]
for pkg in sorted(metadata['packages'],key=lambda p:(p['name'],p['version'])):
 if pkg['id'] not in resolved or pkg['name']=='beek':continue
 base=Path(pkg['manifest_path']).parent
 lines += ['='*72,pkg['name']+' '+pkg['version'],'SPDX: '+str(pkg.get('license') or 'see license file'),'Authors: '+', '.join(pkg.get('authors',[])),'Repository: '+str(pkg.get('repository') or ''),'']
 candidates={f for f in base.iterdir() if f.is_file() and f.name.lower().startswith(('license','licence','copying','notice','copyright'))}
 if pkg.get('license_file'):candidates.add(base/pkg['license_file'])
 for folder in ('licenses','LICENSES','license'):
  if (base/folder).is_dir():candidates.update(f for f in (base/folder).rglob('*') if f.is_file())
 override=root/'resources/windows/licenses'/f"{pkg['name']}-{pkg['version']}.txt"
 if override.is_file():candidates.add(override)
 if not candidates:missing.append(pkg);lines+=['No separate license text shipped in this crate archive; see SPDX declaration above.','']
 for file in sorted(candidates):
  if file.stat().st_size>1024*1024:raise SystemExit('Unexpectedly large license file')
  lines += ['--- '+(file.relative_to(base).as_posix() if file.is_relative_to(base) else 'verified upstream license')+' ---',file.read_text(encoding='utf-8',errors='replace'),'']
args.out.parent.mkdir(parents=True,exist_ok=True);args.out.write_text('\n'.join(lines),encoding='utf-8')
if args.missing_json:args.missing_json.write_text(json.dumps(missing,indent=2),encoding='utf-8')
if args.strict and missing:raise SystemExit('Formal notice inventory incomplete: '+', '.join(p['name']+' '+p['version'] for p in missing))
print('Dependency notice inventory generated; archives without separate text: '+str(len(missing)))
