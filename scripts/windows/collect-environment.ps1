param([Parameter(Mandatory = $true)][string]$OutputFile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Requires actual Windows.' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BeeKDpi {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(Point p, uint flags);
  [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr h, int kind, out uint x, out uint y);
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
}
'@
$dpiContextSet = [BeeKDpi]::SetProcessDpiAwarenessContext([IntPtr](-4))
Add-Type -AssemblyName System.Windows.Forms
$monitors = @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
    $point = New-Object BeeKDpi+Point
    $point.X = $_.Bounds.X + 1
    $point.Y = $_.Bounds.Y + 1
    $handle = [BeeKDpi]::MonitorFromPoint($point, 2)
    [uint32]$x = 0; [uint32]$y = 0
    $dpiResult = [BeeKDpi]::GetDpiForMonitor($handle, 0, [ref]$x, [ref]$y)
    [ordered]@{ name = $_.DeviceName; primary = $_.Primary
        x = $_.Bounds.X; y = $_.Bounds.Y; width = $_.Bounds.Width; height = $_.Bounds.Height
        work_x = $_.WorkingArea.X; work_y = $_.WorkingArea.Y
        work_width = $_.WorkingArea.Width; work_height = $_.WorkingArea.Height
        dpi_x = $x; dpi_y = $y; dpi_hresult = $dpiResult }
})
$os = Get-CimInstance Win32_OperatingSystem
$gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion)
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$sdk = @(Get-ChildItem $sdkRoot -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\fxc.exe') } |
    Sort-Object Name -Descending)
if ($sdk.Count -eq 0) { throw 'Windows SDK with x64 fxc.exe not found. Install Windows SDK before G0.' }
$fxc = Join-Path $sdk[0].FullName 'x64\fxc.exe'
$env:GPUI_FXC_PATH = $fxc
$rust = (& rustc --version) -join ' '
if ($LASTEXITCODE -ne 0) { throw 'rustc is unavailable.' }
$cargo = (& cargo --version) -join ' '
if ($LASTEXITCODE -ne 0) { throw 'cargo is unavailable.' }
$compiler = Get-Command cl.exe -ErrorAction SilentlyContinue
$clPath = if ($compiler) { $compiler.Source } else { '' }
if (-not $clPath) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $installation = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath) -join ''
        if ($installation) {
            $versionFile = Join-Path $installation 'VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt'
            if (Test-Path $versionFile) {
                $toolset = (Get-Content $versionFile -Raw).Trim()
                $clPath = Join-Path $installation ("VC\Tools\MSVC\$toolset\bin\Hostx64\x64\cl.exe")
            }
        }
    }
}
if (-not $clPath -or -not (Test-Path $clPath)) { throw 'MSVC x64 compiler not found. Install C++ Build Tools.' }
$msvc = (Get-Item $clPath).VersionInfo.FileVersion
$cpuArchitectures = @(Get-CimInstance Win32_Processor | Select-Object -ExpandProperty Architecture)
$arch = if ($cpuArchitectures.Count -gt 0 -and @($cpuArchitectures | Where-Object { $_ -ne 9 }).Count -eq 0) { 'X64' }
    elseif (@($cpuArchitectures | Where-Object { $_ -eq 12 }).Count -gt 0) { 'ARM64' } else { 'Other' }
$apps = [ordered]@{ notepad = ''; edge = ''; chrome = '' }
$notepad = @(Get-AppxPackage -Name Microsoft.WindowsNotepad -ErrorAction SilentlyContinue)
if ($notepad.Count -gt 0) { $apps.notepad = $notepad[0].Version.ToString() }
else { $apps.notepad = (Get-Item (Join-Path $env:WINDIR 'System32\notepad.exe')).VersionInfo.FileVersion }
foreach ($location in @((Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe'),
                         (Join-Path $env:ProgramFiles 'Microsoft\Edge\Application\msedge.exe'))) {
    if (Test-Path $location) { $apps.edge = (Get-Item $location).VersionInfo.FileVersion; break }
}
foreach ($location in @((Join-Path $env:ProgramFiles 'Google\Chrome\Application\chrome.exe'),
                         (Join-Path ${env:ProgramFiles(x86)} 'Google\Chrome\Application\chrome.exe'),
                         (Join-Path $env:LOCALAPPDATA 'Google\Chrome\Application\chrome.exe'))) {
    if (Test-Path $location) { $apps.chrome = (Get-Item $location).VersionInfo.FileVersion; break }
}
$environment = [ordered]@{
    os = 'Windows'; os_build = [int]$os.BuildNumber; os_version = $os.Version
    arch = $arch; interactive = [Environment]::UserInteractive
    cpu_architecture_codes = $cpuArchitectures; apps = $apps
    session_id = (Get-Process -Id $PID).SessionId
    rustc = $rust; cargo = $cargo; msvc = $msvc; windows_sdk = $sdk[0].Name; fxc_path = $fxc
    gpu = $gpu; monitors = $monitors; dpi_context_set = $dpiContextSet
    captured_utc = [DateTime]::UtcNow.ToString('o')
}
if ($arch -ne 'X64' -or [int]$os.BuildNumber -lt 22000 -or -not $environment.interactive -or $environment.session_id -eq 0) {
    throw 'Acceptance requires Windows 11 x64 with a logged-in interactive desktop; service/ARM64 is not a pass.'
}
$directory = Split-Path -Parent $OutputFile
if ($directory) { New-Item -ItemType Directory -Force $directory | Out-Null }
[IO.File]::WriteAllText($OutputFile, ($environment | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
Write-Host "Environment written to $OutputFile"
