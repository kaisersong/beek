# Revision 2: network-packaging

Network.m1 行64KiB边界排除CRLF，仍读取中限长，新增边界单测。m2改带generation的覆盖式结果mailbox，不阻塞worker、拒绝迟到结果；generation+mailbox更新在同一结果锁下原子化。m3当前dev版本按core semver比稳定，不对较高dev版本做降级。m4 卸载锁保持至DeinitializeUninstall，保留零字节install.lock，不在收尾提前删根锁/根目录；m5 Run路径先GetLongPathName归一再比较；m6 原产品已由lifecycle::install_guard在每个已安装进程生命周期持有shared read install.lock，独占安装/卸载锁会拒绝其他会话，不需要Global提权；此轮附该实现。m7 ISCC签名命令引号改$q避免原生shell转义，Capabilities异步排空stdout、5s预检上限、不重定向stderr，签名certificate判空；Formal已要求签名uninstaller。发行首版只原生x64：Inno改x64os，程序用IsWow64Process2核对native AMD64。许可清单严格生成已无缺失，保留原始crate作者/SPDX和不可变upstream/标准文本来源，不冒称外部法律意见。

以下只列与首轮快照的真实差异，未出现区域未修改。允许Read补读本附件。G5、正式发布均未通过。

## src/core/stream.rs
```diff
--- round1/src/core/stream.rs
+++ current/src/core/stream.rs
@@ -242,12 +242,12 @@
     let mut finished = false;
     loop {
         let mut line = Vec::new();
         let n = (&mut reader)
-            .take((FRAME + 1) as u64)
+            .take((FRAME + 3) as u64)
             .read_until(b'\n', &mut line)
             .map_err(|_| "读取翻译流失败或超时")?;
-        if n > FRAME {
+        if n > FRAME + 2 {
             return Err("翻译流单行超过64KiB限制".into());
         }
         total += n;
         if total > 4 * 1024 * 1024 {
@@ -260,8 +260,11 @@
             break;
         }
         while matches!(line.last(), Some(b'\n' | b'\r')) {
             line.pop();
+        }
+        if line.len() > FRAME {
+            return Err("翻译流单行超过64KiB限制".into());
         }
         if line.is_empty() {
             if !frame.is_empty() {
                 if consume(&frame, &mut full, &mut finished, &mut on_chunk)? {
@@ -348,5 +351,15 @@
     #[test]
     fn empty_done_is_not_a_translation() {
         assert!(parse(io::Cursor::new(b"data: [DONE]\n\n"), |_| {}).is_err());
     }
-}
+    #[test]
+    fn line_boundary_excludes_crlf_and_rejects_one_extra_content_byte() {
+        for ending in ["\n", "\r\n"] {
+            let comment = format!(":{}{}", "a".repeat(FRAME - 1), ending);
+            let valid = comment
+                + "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
+            assert_eq!(parse(io::Cursor::new(valid), |_| {}).unwrap(), "ok");
+        }
+        assert!(parse(io::Cursor::new(format!(":{}\n", "a".repeat(FRAME))), |_| {}).is_err());
+    }
+}

```

## src/core/release.rs
```diff
--- round1/src/core/release.rs
+++ current/src/core/release.rs
@@ -33,10 +33,12 @@
     if release.draft || release.prerelease {
         return Err("该版本不是稳定发布".into());
     }
     let latest = version(&release.tag_name).ok_or("发布版本格式无效")?;
-    let current = version(current).ok_or("当前版本格式无效")?;
-    if latest <= current {
+    let prerelease = current.contains('-');
+    let current =
+        version(current.split(['-', '+']).next().unwrap_or(current)).ok_or("当前版本格式无效")?;
+    if latest < current || (latest == current && !prerelease) {
         return Ok(None);
     }
     let number = format!("{}.{}.{}", latest[0], latest[1], latest[2]);
     for suffix in ["windows-x64.zip", "windows-x64-setup.exe"] {

```

## src/platform/windows/integration.rs
```diff
--- round1/src/platform/windows/integration.rs
+++ current/src/platform/windows/integration.rs
@@ -3,29 +3,61 @@
     core::w,
     Win32::{System::Registry::*, UI::Shell::*, UI::WindowsAndMessaging::SW_SHOWNORMAL},
 };
 type UpdateWorker = (
-    std::sync::mpsc::SyncSender<()>,
-    std::sync::Mutex<std::sync::mpsc::Receiver<String>>,
+    std::sync::mpsc::SyncSender<u64>,
+    std::sync::Arc<std::sync::Mutex<Option<(u64, String)>>>,
+    std::sync::Arc<std::sync::atomic::AtomicU64>,
 );
 static UPDATES: std::sync::OnceLock<UpdateWorker> = std::sync::OnceLock::new();
 pub fn open_updates() -> bool {
-    let (tx, _) = UPDATES.get_or_init(|| {
+    let (tx, result, current) = UPDATES.get_or_init(|| {
         let (tx, rx) = std::sync::mpsc::sync_channel(1);
-        let (out, result) = std::sync::mpsc::sync_channel(1);
+        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
+        let latest = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
+        let out = result.clone();
+        let current = latest.clone();
         std::thread::spawn(move || {
-            while rx.recv().is_ok() {
-                let status = check_update();
-                if out.send(status).is_err() {
-                    break;
+            while let Ok(id) = rx.recv() {
+                let (status, open) = check_update();
+                let accepted = {
+                    let mut slot = out.lock().unwrap();
+                    if current.load(std::sync::atomic::Ordering::Acquire) == id {
+                        *slot = Some((id, status));
+                        true
+                    } else {
+                        false
+                    }
+                };
+                if accepted && open {
+                    unsafe {
+                        let _ = ShellExecuteW(
+                            None,
+                            w!("open"),
+                            w!("https://github.com/kaisersong/beek/releases"),
+                            None,
+                            None,
+                            SW_SHOWNORMAL,
+                        );
+                    }
                 }
             }
         });
-        (tx, std::sync::Mutex::new(result))
+        (tx, result, latest)
     });
-    tx.try_send(()).is_ok()
+    // UI calls are serialized. Publish the generation before waking the worker.
+    let mut slot = result.lock().unwrap();
+    let previous = current.load(std::sync::atomic::Ordering::Acquire);
+    let id = previous.wrapping_add(1);
+    current.store(id, std::sync::atomic::Ordering::Release);
+    if tx.try_send(id).is_err() {
+        current.store(previous, std::sync::atomic::Ordering::Release);
+        return false;
+    }
+    *slot = None;
+    true
 }
-fn check_update() -> String {
+fn check_update() -> (String, bool) {
     use std::{io::Read, time::Duration};
     let result = (|| -> Result<Option<String>, String> {
         let client = reqwest::blocking::Client::builder()
             .timeout(Duration::from_secs(10))
@@ -56,29 +88,24 @@
         let release = serde_json::from_slice(&bytes).map_err(|_| "更新信息格式无效")?;
         crate::core::release::windows_update(&release, env!("CARGO_PKG_VERSION"))
     })();
     match result {
-        Ok(None) => "当前已是最新 Windows 版本".into(),
-        Ok(Some(version)) => {
-            unsafe {
-                let _ = ShellExecuteW(
-                    None,
-                    w!("open"),
-                    w!("https://github.com/kaisersong/beek/releases"),
-                    None,
-                    None,
-                    SW_SHOWNORMAL,
-                );
-            }
-            format!("发现 {version}，已打开固定发布页；请下载并检查签名后安装")
-        }
-        Err(error) => error,
+        Ok(None) => ("当前已是最新 Windows 版本".into(), false),
+        Ok(Some(version)) => (
+            format!("发现 {version}，已打开固定发布页；请下载并检查签名后安装"),
+            true,
+        ),
+        Err(error) => (error, false),
     }
 }
 pub fn update_result() -> Option<String> {
-    UPDATES
-        .get()
-        .and_then(|(_, rx)| rx.lock().ok()?.try_recv().ok())
+    let (_, result, current) = UPDATES.get()?;
+    let (id, status) = result.lock().ok()?.take()?;
+    if current.load(std::sync::atomic::Ordering::Acquire) == id {
+        Some(status)
+    } else {
+        None
+    }
 }
 pub fn autostart(enabled: bool) -> Result<(), String> {
     unsafe {
         let mut key = HKEY::default();

```

## scripts/windows/package.ps1
```diff
--- round1/scripts/windows/package.ps1
+++ current/scripts/windows/package.ps1
@@ -6,12 +6,12 @@
 if(-not $OutputDirectory){$OutputDirectory=Join-Path $repo 'dist\windows'}
 $version=([regex]::Match([IO.File]::ReadAllText((Join-Path $repo 'Cargo.toml')),'(?m)^version = "([^"]+)"')).Groups[1].Value
 if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid Cargo version'}
 $exe=Join-Path $TargetDirectory 'beek.exe';$helper=Join-Path $TargetDirectory 'beek-selection-host.exe'
-function Capabilities([string]$Path){$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=(Resolve-Path $Path).Path;$p.StartInfo.Arguments='--capabilities';$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$true;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$json=$p.StandardOutput.ReadToEnd();$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Capability command failed'};$p.Dispose();$json|ConvertFrom-Json}
+function Capabilities([string]$Path){$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=(Resolve-Path $Path).Path;$p.StartInfo.Arguments='--capabilities';$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$false;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$reading=$p.StandardOutput.ReadToEndAsync();if(-not $p.WaitForExit(5000)){$p.Kill();throw 'Capability preflight exceeded 5 seconds'};$json=$reading.GetAwaiter().GetResult();if($p.ExitCode -ne 0){throw 'Capability command failed'};$p.Dispose();$json|ConvertFrom-Json}
 $main=Capabilities $exe;$hostInfo=Capabilities $helper
 if($main.build_id -ne $hostInfo.build_id -or $main.protocol -ne $hostInfo.protocol -or $main.binary -ne 'main' -or $hostInfo.binary -ne 'selection-host'){throw 'Main/helper pair mismatch'}
-foreach($info in @($main,$hostInfo)){if($info.os -ne 'windows' -or $info.arch -ne 'x86_64' -or $info.version -ne $version -or $info.debug_assertions -or $info.test_fixtures){throw 'Only matching non-fixture release x64 builds can be packaged'}}
+foreach($info in @($main,$hostInfo)){if($info.os -ne 'windows' -or $info.arch -ne 'x86_64' -or $info.native_machine -ne 34404 -or $info.version -ne $version -or $info.debug_assertions -or $info.test_fixtures){throw 'Only matching non-fixture release x64 builds can be packaged'}}
 if($Formal -and (-not $PublisherThumbprint -or -not $SignTool)){throw 'Formal packages require explicit signer thumbprint and signtool'}
 if(Test-Path $OutputDirectory){throw 'Use a fresh output directory; do not mix package generations'}
 $null=New-Item -ItemType Directory $OutputDirectory -Force;$OutputDirectory=(Resolve-Path $OutputDirectory).Path
 $payload=Join-Path $OutputDirectory 'payload';$null=New-Item -ItemType Directory $payload
@@ -23,16 +23,16 @@
 $licenseArgs=@((Join-Path $PSScriptRoot 'licenses.py'),'--out',(Join-Path $payload 'THIRD-PARTY-NOTICES.txt'));if($Formal){$licenseArgs+='--strict'};& python @licenseArgs
 if($LASTEXITCODE -ne 0){throw 'Dependency license inventory failed'}
 function Sign([string]$Path){
  if($Formal){& $SignTool sign /sha1 $PublisherThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $Path;if($LASTEXITCODE -ne 0){throw 'Authenticode signing failed'}
- $signature=Get-AuthenticodeSignature $Path;if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $PublisherThumbprint){throw 'Signature publisher verification failed'}}
+ $signature=Get-AuthenticodeSignature $Path;if($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $PublisherThumbprint){throw 'Signature publisher verification failed'}}
 }
 Sign (Join-Path $payload 'beek.exe');Sign (Join-Path $payload 'beek-selection-host.exe')
 $files=@(Get-ChildItem $payload -File|Sort-Object Name|ForEach-Object {@{name=$_.Name;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower();bytes=$_.Length}})
 $manifest=@{schema=1;version=$version;build_id=$main.build_id;protocol=$main.protocol;mode='portable';distribution=$(if($Formal){'formal-signed'}else{'internal-preview-unsigned'});files=$files}
 [IO.File]::WriteAllText((Join-Path $payload 'manifest.json'),($manifest|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 $marker=Join-Path $repo 'packaging\windows\installed.marker'
-$signArgs=@();if($Formal){if($PublisherThumbprint -notmatch '^[a-fA-F0-9]{40}$'){throw 'Invalid certificate thumbprint'};$signArgs=@('/DFormalSigning=1',('/Sbeek="'+$SignTool+'" sign /sha1 '+$PublisherThumbprint+' /fd SHA256 /tr '+$TimestampUrl+' /td SHA256 $f'))}
+$signArgs=@();if($Formal){if($PublisherThumbprint -notmatch '^[a-fA-F0-9]{40}$'){throw 'Invalid certificate thumbprint'};$signArgs=@('/DFormalSigning=1',('/Sbeek=$q'+$SignTool+'$q sign /sha1 '+$PublisherThumbprint+' /fd SHA256 /tr '+$TimestampUrl+' /td SHA256 $f'))}
 & $Iscc @signArgs "/DAppVersion=$version" "/DBuildId=$($main.build_id)" "/DPayloadDir=$payload" "/DOutputPath=$OutputDirectory" "/DMarkerPath=$marker" (Join-Path $repo 'packaging\windows\beek.iss')
 if($LASTEXITCODE -ne 0){throw 'Installer compilation failed'}
 $setup=Join-Path $OutputDirectory "BeeK-v$version-windows-x64-setup.exe";Sign $setup
 if($Formal){$records=@{};foreach($file in @((Join-Path $payload 'beek.exe'),(Join-Path $payload 'beek-selection-host.exe'),$setup)){$signature=Get-AuthenticodeSignature $file;$records[[IO.Path]::GetFileName($file)]=@{status=$signature.Status.ToString();thumbprint=$signature.SignerCertificate.Thumbprint;sha256=(Get-FileHash $file -Algorithm SHA256).Hash.ToLower()}};@{schema=1;expected_thumbprint=$PublisherThumbprint;files=$records;signed_uninstaller=$true}|ConvertTo-Json -Depth 6|ForEach-Object {[IO.File]::WriteAllText((Join-Path $OutputDirectory 'windows-signatures.json'),$_,[Text.UTF8Encoding]::new($false))}}

```

## packaging/windows/beek.iss
```diff
--- round1/packaging/windows/beek.iss
+++ current/packaging/windows/beek.iss
@@ -20,10 +20,10 @@
 AppPublisherURL=https://github.com/kaisersong/beek
 DefaultDirName={localappdata}\Programs\BeeK
 DefaultGroupName=BeeK
 PrivilegesRequired=lowest
-ArchitecturesAllowed=x64compatible
-ArchitecturesInstallIn64BitMode=x64compatible
+ArchitecturesAllowed=x64os
+ArchitecturesInstallIn64BitMode=x64os
 MinVersion=10.0.22000
 DisableProgramGroupPage=yes
 UninstallDisplayIcon={app}\versions\{#PayloadName}\beek.exe
 OutputDir={#OutputPath}
@@ -55,13 +55,24 @@
 [Run]
 Filename: "{app}\versions\{#PayloadName}\beek.exe"; Description: "Launch BeeK"; Flags: nowait postinstall skipifsilent unchecked
 [UninstallDelete]
 Type: filesandordirs; Name: "{app}\versions"
-Type: files; Name: "{app}\install.lock"
 [Code]
 var InstallLock: THandle; PurgeData: Boolean;
 function CreateFileW(Name: String; Access, Share: LongWord; Security: Integer; Creation, Flags: LongWord; Template: Integer): THandle; external 'CreateFileW@kernel32.dll stdcall';
 function CloseHandle(Handle: THandle): Boolean; external 'CloseHandle@kernel32.dll stdcall';
+function GetLongPathNameW(Path: String; Buffer: String; Length: LongWord): LongWord; external 'GetLongPathNameW@kernel32.dll stdcall';
+function NormalPath(Path: String): String;
+var Buffer: String; N: LongWord;
+begin SetLength(Buffer, 32768); N := GetLongPathNameW(Path, Buffer, 32768); if (N > 0) and (N < 32768) then begin SetLength(Buffer, N); Result := Lowercase(Buffer); end else Result := Lowercase(ExpandFileName(Path)); end;
+function OwnsRun(Value: String): Boolean;
+var Path, Rest: String; N: Integer;
+begin
+ Result := False; if Value = '' then exit;
+ if Value[1] = '"' then begin Rest := Copy(Value, 2, Length(Value)); N := Pos('"', Rest); if N = 0 then exit; Path := Copy(Rest, 1, N-1); end
+ else begin N := Pos(' ', Value); if N = 0 then Path := Value else Path := Copy(Value, 1, N-1); end;
+ Result := Pos(NormalPath(ExpandConstant('{app}')) + '\', NormalPath(Path)) = 1;
+end;
 function PrepareToInstall(var NeedsRestart: Boolean): String;
 begin
  Result := '';
  if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock);
@@ -74,9 +85,9 @@
 begin
  if CurStep = ssPostInstall then begin
   // Only after both immutable versioned executables are fully extracted do
   // shortcuts / the existing opted-in Run entry switch to the new pair.
-  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and (Pos('"' + ExpandConstant('{app}') + '\', Old) = 1) then
+  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and OwnsRun(Old) then
     RegWriteStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', '"' + ExpandConstant('{app}\versions\{#PayloadName}\beek.exe') + '" --autostart');
   if InstallLock <> 0 then begin CloseHandle(InstallLock); InstallLock := 0; end;
  end;
 end;
@@ -97,8 +108,10 @@
   if InstallLock = -1 then RaiseException('BeeK started during uninstall. Quit it and retry.');
   if PurgeData then begin
    if not Exec(ExpandConstant('{app}\versions\{#PayloadName}\beek.exe'), '--purge-user-data', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then RaiseException('BeeK data cleanup failed; files and remaining data preserved.');
   end;
-  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and (Pos('"' + ExpandConstant('{app}') + '\', Old) = 1) then RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK');
+  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and OwnsRun(Old) then RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK');
  end;
- if CurUninstallStep = usPostUninstall then begin if InstallLock <> 0 then begin CloseHandle(InstallLock); InstallLock := 0; end; DeleteFile(ExpandConstant('{app}\install.lock')); end;
 end;
+
+procedure DeinitializeUninstall;
+begin if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock); end;

```

## 已有的安装shared lock
```rust
pub fn install_guard() -> Result<Option<fs::File>, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
    let dir = exe.parent().ok_or("程序目录不可用")?;
    if !dir.join("installed.marker").exists() {
        return Ok(None);
    }
    let root = dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|v| v == "versions"))
        .and_then(|p| p.parent())
        .ok_or("安装目录结构无效")?;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(root.join("install.lock"))
        .map(Some)
        .map_err(|_| "安装/卸载正在进行，请完成后重试".into())
}

```
