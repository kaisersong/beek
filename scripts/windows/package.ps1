param([string]$TargetDirectory='',[string]$OutputDirectory='',[Parameter(Mandatory=$true)][string]$Iscc,[switch]$Formal,[switch]$PublicPreview,[string]$PublisherThumbprint='', [string]$SignTool='',[string]$TimestampUrl='http://timestamp.digicert.com')
$ErrorActionPreference='Stop'; Set-StrictMode -Version Latest
if($Formal -and $PublicPreview){throw 'Choose signed release or unsigned public preview'}
$PublisherThumbprint=$PublisherThumbprint.ToUpperInvariant()
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if(-not $TargetDirectory){$TargetDirectory=Join-Path $repo 'target\release'}
if(-not $OutputDirectory){$OutputDirectory=Join-Path $repo 'dist\windows'}
$version=([regex]::Match([IO.File]::ReadAllText((Join-Path $repo 'Cargo.toml')),'(?m)^version = "([^"]+)"')).Groups[1].Value
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid Cargo version'}
$exe=Join-Path $TargetDirectory 'beek.exe';$helper=Join-Path $TargetDirectory 'beek-selection-host.exe'
function Capabilities([string]$Path){$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=(Resolve-Path $Path).Path;$p.StartInfo.Arguments='--capabilities';$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$false;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$reading=$p.StandardOutput.ReadToEndAsync();if(-not $p.WaitForExit(5000)){$p.Kill();throw 'Capability preflight exceeded 5 seconds'};$json=$reading.GetAwaiter().GetResult();if($p.ExitCode -ne 0){throw 'Capability command failed'};$p.Dispose();$json|ConvertFrom-Json}
$main=Capabilities $exe;$hostInfo=Capabilities $helper
if($main.build_id -ne $hostInfo.build_id -or $main.protocol -ne $hostInfo.protocol -or $main.binary -ne 'main' -or $hostInfo.binary -ne 'selection-host'){throw 'Main/helper pair mismatch'}
foreach($info in @($main,$hostInfo)){if($info.os -ne 'windows' -or $info.arch -ne 'x86_64' -or $info.native_machine -ne 34404 -or $info.version -ne $version -or $info.debug_assertions -or $info.test_fixtures){throw 'Only matching non-fixture release x64 builds can be packaged'}}
if($Formal -and (-not $PublisherThumbprint -or -not $SignTool)){throw 'Formal packages require explicit signer thumbprint and signtool'}
if(Test-Path $OutputDirectory){throw 'Use a fresh output directory; do not mix package generations'}
$null=New-Item -ItemType Directory $OutputDirectory -Force;$OutputDirectory=(Resolve-Path $OutputDirectory).Path
$payload=Join-Path $OutputDirectory 'payload';$null=New-Item -ItemType Directory $payload
Copy-Item $exe,$helper $payload
Copy-Item (Join-Path $repo 'LICENSE*') $payload
Copy-Item (Join-Path $repo 'resources\GPUI-*') $payload
Copy-Item (Join-Path $repo 'packaging\windows\README-WINDOWS.txt') $payload
# Source license inventory is generated from the exact locked dependency graph.
$licenseArgs=@((Join-Path $PSScriptRoot 'licenses.py'),'--out',(Join-Path $payload 'THIRD-PARTY-NOTICES.txt'));if($Formal){$licenseArgs+='--strict'};& python @licenseArgs
if($LASTEXITCODE -ne 0){throw 'Dependency license inventory failed'}
function Sign([string]$Path){
 if($Formal){& $SignTool sign /sha1 $PublisherThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $Path;if($LASTEXITCODE -ne 0){throw 'Authenticode signing failed'}
 $signature=Get-AuthenticodeSignature $Path;if($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $PublisherThumbprint){throw 'Signature publisher verification failed'}}
}
Sign (Join-Path $payload 'beek.exe');Sign (Join-Path $payload 'beek-selection-host.exe')
$files=@(Get-ChildItem $payload -File|Sort-Object Name|ForEach-Object {@{name=$_.Name;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower();bytes=$_.Length}})
$manifest=@{schema=1;version=$version;build_id=$main.build_id;protocol=$main.protocol;mode='portable';distribution=$(if($Formal){'formal-signed'}elseif($PublicPreview){'public-preview-unsigned'}else{'internal-preview-unsigned'});files=$files}
[IO.File]::WriteAllText((Join-Path $payload 'manifest.json'),($manifest|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$marker=Join-Path $repo 'packaging\windows\installed.marker'
$signArgs=@();if($Formal){if($PublisherThumbprint -notmatch '^[a-fA-F0-9]{40}$'){throw 'Invalid certificate thumbprint'};$signArgs=@('/DFormalSigning=1',('/Sbeek=$q'+$SignTool+'$q sign /sha1 '+$PublisherThumbprint+' /fd SHA256 /tr '+$TimestampUrl+' /td SHA256 $f'))}
& $Iscc @signArgs "/DAppVersion=$version" "/DBuildId=$($main.build_id)" "/DPayloadDir=$payload" "/DOutputPath=$OutputDirectory" "/DMarkerPath=$marker" (Join-Path $repo 'packaging\windows\beek.iss')
if($LASTEXITCODE -ne 0){throw 'Installer compilation failed'}
$setup=Join-Path $OutputDirectory "BeeK-v$version-windows-x64-setup.exe";Sign $setup
if($Formal){$records=@{};foreach($file in @((Join-Path $payload 'beek.exe'),(Join-Path $payload 'beek-selection-host.exe'),$setup)){$signature=Get-AuthenticodeSignature $file;$records[[IO.Path]::GetFileName($file)]=@{status=$signature.Status.ToString();thumbprint=$signature.SignerCertificate.Thumbprint;sha256=(Get-FileHash $file -Algorithm SHA256).Hash.ToLower()}};@{schema=1;expected_thumbprint=$PublisherThumbprint;files=$records;signed_uninstaller=$true}|ConvertTo-Json -Depth 6|ForEach-Object {[IO.File]::WriteAllText((Join-Path $OutputDirectory 'windows-signatures.json'),$_,[Text.UTF8Encoding]::new($false))}}
$zip=Join-Path $OutputDirectory "BeeK-v$version-windows-x64.zip"
Compress-Archive -Path (Join-Path $payload '*') -DestinationPath $zip
foreach($file in @($zip,$setup)){[IO.File]::WriteAllText(($file+'.sha256'),((Get-FileHash $file -Algorithm SHA256).Hash.ToLower()+'  '+[IO.Path]::GetFileName($file)+"`n"),[Text.UTF8Encoding]::new($false))}
Write-Host "Created $($manifest.distribution) packages in $OutputDirectory. Packaging is not G5 acceptance."
