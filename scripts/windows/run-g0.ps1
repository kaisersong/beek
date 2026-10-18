param([string]$OutputDirectory = '', [switch]$BuildOnly)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$probe = Join-Path $repo 'tools\windows-probe'
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $repo ('test-results\g0-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
}
if (Test-Path $OutputDirectory) { throw 'Use a new evidence directory for each run.' }
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$OutputDirectory = (Resolve-Path $OutputDirectory).Path
$logs = Join-Path $OutputDirectory 'logs'
New-Item -ItemType Directory $logs | Out-Null

function Invoke-Cargo([string[]]$CargoArguments, [string]$Log) {
    # Windows PowerShell 5.1 represents native stderr as ErrorRecord objects.
    # Capture it without treating an ordinary cargo progress line as a failure.
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & cargo @CargoArguments *> $Log
    $code = $LASTEXITCODE
    $ErrorActionPreference = $savedPreference
    if ($code -ne 0) { throw "cargo failed ($code); inspect $Log" }
}

Push-Location $probe
try {
    if (-not (Test-Path 'Cargo.lock')) { throw 'Committed G0 Cargo.lock missing. Do not resolve new dependencies during acceptance.' }
    & python --version
    if ($LASTEXITCODE -ne 0) { throw 'Python 3.9+ is required.' }
    & (Join-Path $PSScriptRoot 'collect-environment.ps1') -OutputFile (Join-Path $OutputDirectory 'environment.json')
    Invoke-Cargo -CargoArguments @('build', '--locked') -Log (Join-Path $logs 'build-debug.log')
    Invoke-Cargo -CargoArguments @('build', '--locked', '--release') -Log (Join-Path $logs 'build-release.log')
    Invoke-Cargo -CargoArguments @('test', '--locked') -Log (Join-Path $logs 'cargo-test.log')
    $exe = Join-Path $probe 'target\release\beek-windows-probe.exe'
    if (-not (Test-Path $exe)) { throw 'Expected release probe binary missing.' }
    $run = Join-Path $OutputDirectory 'run.json'
    & python (Join-Path $PSScriptRoot 'evidence.py') create --stage G0 --out $run --environment (Join-Path $OutputDirectory 'environment.json') --artifact $exe
    if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize acceptance record.' }
    & python (Join-Path $PSScriptRoot 'evidence.py') record --run $run --case G0-01-BUILD --status pass --actual 'Locked debug/release builds and cargo tests completed with exit 0.' --evidence 'logs/build-debug.log' --evidence 'logs/build-release.log' --evidence 'logs/cargo-test.log'
    if ($LASTEXITCODE -ne 0) { throw 'Cannot record build evidence.' }
    if ($BuildOnly) {
        Write-Host 'Build checks complete; interactive G0 cases remain NOT RUN.'
        return
    }
    Write-Host 'Follow docs/windows/windows-test-plan.md G0. Use only public fixture text.'
    $trace = Join-Path $logs 'probe.jsonl'
    # Start the exact copy whose digest was entered in run.json.
    $testedExe = Join-Path $OutputDirectory 'artifacts\beek-windows-probe.exe'
    $process = Start-Process -FilePath $testedExe -ArgumentList ('"' + $trace + '"') -PassThru
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Probe exited with $($process.ExitCode); preserve evidence and mark failures." }
    & python (Join-Path $PSScriptRoot 'trace_check.py') $trace
    $traceCode = $LASTEXITCODE
    Write-Host "Structural trace check exit: $traceCode. Complete per-case actual results/screenshots; this is not a G0 PASS."
    Write-Host "Acceptance record: $run"
    Write-Host "Gate: python scripts/windows/evidence.py validate --run `"$run`""
}
finally { Pop-Location }
