param([Parameter(Mandatory = $true)][string]$OutputFile, [ValidateRange(300, 3600)][int]$Seconds = 300)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$session = (Get-Process -Id $PID).SessionId
$previous = @{}
$samples = New-Object 'Collections.Generic.List[object]'
$clock = [Diagnostics.Stopwatch]::StartNew()
$previousTime = 0.0
for ($i = 0; $i -le $Seconds; $i++) {
    $processes = @(Get-Process -Name 'beek', 'beek-selection-host' -ErrorAction SilentlyContinue |
        Where-Object { $_.SessionId -eq $session })
    if (@($processes | Where-Object ProcessName -eq 'beek').Count -ne 1) {
        throw 'Idle measurement requires exactly one live BeeK main process in this session.'
    }
    $now = $clock.Elapsed.TotalSeconds
    $cpu = 0.0; $rss = 0.0; $handles = 0; $threads = 0
    $current = @{}
    foreach ($process in $processes) {
        $key = "$($process.Id):$($process.StartTime.ToUniversalTime().Ticks)"
        $totalCpu = $process.TotalProcessorTime.TotalSeconds
        $current[$key] = $totalCpu
        if ($previous.ContainsKey($key)) { $cpu += [Math]::Max(0, $totalCpu - $previous[$key]) }
        elseif ($i -gt 0) { $cpu += $totalCpu }
        $rss += $process.WorkingSet64 / 1MB
        $handles += $process.HandleCount
        $threads += $process.Threads.Count
    }
    if ($i -gt 0) {
        $samples.Add([pscustomobject]@{
            elapsed_s = [Math]::Round($now, 3)
            sample_interval_s = $now - $previousTime
            cpu_percent_single_core = 100 * $cpu / ($now - $previousTime)
            working_set_mib = $rss; handles = $handles; threads = $threads
            processes = $processes.Count
        })
    }
    $previous = $current; $previousTime = $now
    if ($i -lt $Seconds) { Start-Sleep -Seconds 1 }
}
$samples | Export-Csv -NoTypeInformation -Encoding UTF8 $OutputFile
$elapsed = ($samples | Measure-Object sample_interval_s -Sum).Sum
$cpuSeconds = ($samples | ForEach-Object { $_.cpu_percent_single_core * $_.sample_interval_s / 100 } | Measure-Object -Sum).Sum
$averageCpu = 100 * $cpuSeconds / $elapsed
$maxRss = ($samples | Measure-Object working_set_mib -Maximum).Maximum
Write-Host "CPU single-core weighted average: $averageCpu%; max total working set: $maxRss MiB"
@{schema=1;passed=($averageCpu -le 1 -and $maxRss -le 150);session_id=$session;seconds=$elapsed;samples=$samples.Count;cpu_percent_single_core=$averageCpu;max_total_working_set_mib=$maxRss;last_process_count=$processes.Count} | ConvertTo-Json | ForEach-Object {[IO.File]::WriteAllText(($OutputFile+'.summary.json'),$_,[Text.UTF8Encoding]::new($false))}
if ($averageCpu -gt 1 -or $maxRss -gt 150) { throw 'Idle resource target exceeded; preserve samples and investigate.' }
