#!/usr/bin/env python3
"""Drive the fixture-only Interactive desktop agent locally or through SSH."""
import argparse,base64,json,os,re,subprocess,time,uuid
from pathlib import Path
class Client:
 def __init__(self,workspace,host=None,output=None):
  self.workspace=workspace.rstrip('\\/');self.host=host;self.output=Path(output) if output else None
  if host and not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.@:-]*',host):raise ValueError('invalid SSH host')
 def remote(self,script):
  encoded=base64.b64encode(script.encode('utf-16-le')).decode();r=subprocess.run(['ssh','-o','BatchMode=yes','-o','ConnectTimeout=10',self.host,'powershell','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded],capture_output=True,timeout=25,check=True)
  return json.loads(base64.b64decode(r.stdout.strip()).decode('utf-8-sig'))
 def action(self,action,timeout=35,**values):
  if not self.host and os.name!='nt':raise RuntimeError('local desktop control requires Windows; use --host for SSH')
  identifier=uuid.uuid4().hex;request=dict(id=identifier,action=action,**values);control=self.workspace+'\\remote-evidence\\desktop';body=json.dumps(request,ensure_ascii=False).encode();name=identifier+'.json'
  if self.host:
   root=control.replace("'","''");data=base64.b64encode(body).decode()
   self.remote("$ErrorActionPreference='Stop';$dir='"+root+"';$p=Join-Path $dir 'inbox\\"+name+"';[IO.File]::WriteAllBytes(($p+'.upload'),[Convert]::FromBase64String('"+data+"'));Move-Item ($p+'.upload') $p;[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('{\"queued\":true}'))")
  else:
   folder=Path(control.replace('\\',os.sep));p=folder/'inbox'/name;p.with_suffix('.upload').write_bytes(body);p.with_suffix('.upload').replace(p)
  deadline=time.monotonic()+timeout
  while time.monotonic()<deadline:
   if self.host:
    value=self.remote("$p=Join-Path '"+root+"' 'outbox\\"+name+"';$text=if(Test-Path $p){[IO.File]::ReadAllText($p)}else{'{\"pending\":true}'};[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($text))")
   else:
    p=folder/'outbox'/name;value=json.loads(p.read_text(encoding='utf-8-sig')) if p.exists() else {'pending':True}
   if not value.get('pending'):
    if self.output:self.output.mkdir(parents=True,exist_ok=True);(self.output/name).write_text(json.dumps({'request':request,'response':value},ensure_ascii=False,indent=2),encoding='utf-8')
    return value
   time.sleep(.1)
  raise TimeoutError(action)
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--workspace',required=True);p.add_argument('--host');p.add_argument('--output',type=Path);p.add_argument('--timeout',type=float,default=35);p.add_argument('action');p.add_argument('arguments',nargs='?',default='{}');a=p.parse_args();print(json.dumps(Client(a.workspace,a.host,a.output).action(a.action,a.timeout,**json.loads(a.arguments)),ensure_ascii=False,indent=2))
if __name__=='__main__':main()
