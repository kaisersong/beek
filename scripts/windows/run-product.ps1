param([string]$OutputDirectory='', [switch]$BuildOnly)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if((Get-Process -Id $PID).SessionId -eq 0){throw 'Use a logged-in ordinary Windows desktop; SSH/service session is not acceptance.'}
if(-not $OutputDirectory){$OutputDirectory=Join-Path $repo ('test-results\product-'+(Get-Date -Format 'yyyyMMdd-HHmmss-fff'))}
if(Test-Path $OutputDirectory){throw 'Use a new evidence directory'}
$null=New-Item -ItemType Directory $OutputDirectory;$OutputDirectory=(Resolve-Path $OutputDirectory).Path
$logs=Join-Path $OutputDirectory 'logs';$null=New-Item -ItemType Directory $logs
function Invoke-Cargo([string[]]$Arguments,[string]$Name){$saved=$ErrorActionPreference;$ErrorActionPreference='Continue';& cargo.exe @Arguments *> (Join-Path $logs $Name);$code=$LASTEXITCODE;$ErrorActionPreference=$saved;if($code -ne 0){throw "cargo $Name failed ($code)"}}
function JsonCommand([string]$File,[string]$Arguments){$p=[Diagnostics.Process]::new();try{$p.StartInfo.FileName=$File;$p.StartInfo.Arguments=$Arguments;$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$reading=$p.StandardOutput.ReadToEndAsync();if(-not $p.WaitForExit(5000)){$p.Kill();throw 'Binary preflight exceeded 5 seconds'};$text=$reading.GetAwaiter().GetResult();if($p.ExitCode -ne 0){throw 'Binary preflight failed'};$text|ConvertFrom-Json}finally{$p.Dispose()}}
Push-Location $repo
try{
 $envFile=Join-Path $OutputDirectory 'environment.json';& (Join-Path $PSScriptRoot 'collect-environment.ps1') -OutputFile $envFile
 Invoke-Cargo @('check','--locked','--all-targets') 'cargo-check.log'
 Invoke-Cargo @('test','--locked','--lib','--','--include-ignored','--test-threads=1') 'cargo-test-interactive.log'
 Invoke-Cargo @('build','--locked','--bins') 'cargo-debug.log'
 Invoke-Cargo @('build','--locked','--release','--bin','beek','--bin','beek-selection-host') 'cargo-release.log'
 $target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{Join-Path $repo 'target'}
 $main=Join-Path $target 'release\beek.exe';$helper=Join-Path $target 'release\beek-selection-host.exe'
 $caps=@{main=(JsonCommand $main '--capabilities');'selection-host'=(JsonCommand $helper '--capabilities')}
 foreach($kind in @('main','selection-host')){$file=if($kind -eq 'main'){$main}else{$helper};$caps[$kind]|Add-Member -NotePropertyName sha256 -NotePropertyValue (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()}
 $capsFile=Join-Path $OutputDirectory 'capabilities.json';[IO.File]::WriteAllText($capsFile,($caps|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 Push-Location $env:TEMP
 try{$pair=JsonCommand $main '--verify-pair';if(-not $pair.paired){throw 'Helper pairing depends on cwd or failed'}}finally{Pop-Location}
 foreach($stage in @('G1','G2','G3','G4','G5')){
  $run=Join-Path $OutputDirectory ($stage+'\run.json')
  & python (Join-Path $PSScriptRoot 'evidence.py') create --stage $stage --out $run --environment $envFile --artifact $main --helper-artifact $helper --capabilities $capsFile
  if($LASTEXITCODE -ne 0){throw 'Evidence initialization failed'}
 }
 $g1=Join-Path $OutputDirectory 'G1';Copy-Item $logs (Join-Path $g1 'logs') -Recurse
 & python (Join-Path $PSScriptRoot 'evidence.py') record --run (Join-Path $g1 'run.json') --case G1-01-BUILD-WIN --status pass --actual 'Actual interactive Windows locked check, unit tests including real vault rollback, debug and release binary builds exited zero.' --evidence logs/cargo-check.log --evidence logs/cargo-test-interactive.log --evidence logs/cargo-debug.log --evidence logs/cargo-release.log
 if($LASTEXITCODE -ne 0){throw 'Build evidence recording failed'}
 if(-not $BuildOnly){Start-Process (Join-Path $g1 'artifacts\beek.exe')}
 Write-Host "Evidence initialized: $OutputDirectory. Only the Windows build case is recorded PASS; every remaining case needs actual evidence. Full G0 and G1-G5 remain gated."
}finally{Pop-Location}
