#ifndef AppVersion
  #error AppVersion required
#endif
#ifndef BuildId
  #error BuildId required
#endif
#ifndef PayloadDir
  #error PayloadDir required
#endif
#ifndef OutputPath
  #error OutputPath required
#endif
#define AppGuid "{8E674477-64D4-4AF7-B0E6-C64A5A6748F3}"
#define PayloadName AppVersion + "-" + BuildId
[Setup]
AppId={{8E674477-64D4-4AF7-B0E6-C64A5A6748F3}
AppName=BeeK
AppVersion={#AppVersion}
AppPublisher=BeeK
AppPublisherURL=https://github.com/kaisersong/beek
DefaultDirName={localappdata}\Programs\BeeK
DefaultGroupName=BeeK
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0.22000
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\versions\{#PayloadName}\beek.exe
OutputDir={#OutputPath}
OutputBaseFilename=BeeK-v{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
AppMutex=Local\BeeK-Running
SetupLogging=yes
#ifdef FormalSigning
SignedUninstaller=yes
SignTool=beek
#else
SignedUninstaller=no
#endif
WizardStyle=modern
[Files]
Source: "{#MarkerPath}"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\beek.exe"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\beek-selection-host.exe"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\LICENSE*"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\GPUI-*"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\manifest.json"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\README-WINDOWS.txt"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
[Icons]
Name: "{autoprograms}\BeeK"; Filename: "{app}\versions\{#PayloadName}\beek.exe"
[Run]
Filename: "{app}\versions\{#PayloadName}\beek.exe"; Description: "Launch BeeK"; Flags: nowait postinstall skipifsilent unchecked
[UninstallDelete]
Type: filesandordirs; Name: "{app}\versions"
[Code]
var InstallLock: THandle; PurgeData: Boolean;
function CreateFileW(Name: String; Access, Share: LongWord; Security: Integer; Creation, Flags: LongWord; Template: Integer): THandle; external 'CreateFileW@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean; external 'CloseHandle@kernel32.dll stdcall';
function GetLongPathNameW(Path: String; Buffer: String; Length: LongWord): LongWord; external 'GetLongPathNameW@kernel32.dll stdcall';
function NormalPath(Path: String): String;
var Buffer: String; N: LongWord;
begin SetLength(Buffer, 32768); N := GetLongPathNameW(Path, Buffer, 32768); if (N > 0) and (N < 32768) then begin SetLength(Buffer, N); Result := Lowercase(Buffer); end else Result := Lowercase(ExpandFileName(Path)); end;
function OwnsRun(Value: String): Boolean;
var Path, Rest: String; N: Integer;
begin
 Result := False; if Value = '' then exit;
 if Value[1] = '"' then begin Rest := Copy(Value, 2, Length(Value)); N := Pos('"', Rest); if N = 0 then exit; Path := Copy(Rest, 1, N-1); end
 else begin N := Pos(' ', Value); if N = 0 then Path := Value else Path := Copy(Value, 1, N-1); end;
 Result := Pos(NormalPath(ExpandConstant('{app}')) + '\', NormalPath(Path)) = 1;
end;
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
 Result := '';
 if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock);
 if not ForceDirectories(ExpandConstant('{app}')) then begin Result := 'Cannot create BeeK directory.'; exit; end;
 InstallLock := CreateFileW(ExpandConstant('{app}\install.lock'), $C0000000, 0, 0, 4, 0, 0);
 if InstallLock = -1 then begin Result := 'BeeK is running or another installer is active. Quit BeeK from its tray menu and retry. No files have been replaced.'; end;
end;
procedure CurStepChanged(CurStep: TSetupStep);
var Old: String;
begin
 if CurStep = ssPostInstall then begin
  // Only after both immutable versioned executables are fully extracted do
  // shortcuts / the existing opted-in Run entry switch to the new pair.
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and OwnsRun(Old) then
    RegWriteStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', '"' + ExpandConstant('{app}\versions\{#PayloadName}\beek.exe') + '" --autostart');
  if InstallLock <> 0 then begin CloseHandle(InstallLock); InstallLock := 0; end;
 end;
end;
procedure DeinitializeSetup;
begin if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock); end;
function InitializeUninstall: Boolean;
begin
 PurgeData := False;
 Result := not CheckForMutexes('Local\BeeK-Running');
 if not Result then begin MsgBox('Quit BeeK from its tray menu before uninstalling. No process will be forcibly terminated.', mbError, MB_OK); exit; end;
 if not UninstallSilent then PurgeData := MsgBox('Also permanently delete this Windows user''s BeeK settings, credentials and logs? Choose No to preserve them.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
end;
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var Code: Integer; Old: String;
begin
 if CurUninstallStep = usUninstall then begin
  InstallLock := CreateFileW(ExpandConstant('{app}\install.lock'), $C0000000, 0, 0, 4, 0, 0);
  if InstallLock = -1 then RaiseException('BeeK started during uninstall. Quit it and retry.');
  if PurgeData then begin
   if not Exec(ExpandConstant('{app}\versions\{#PayloadName}\beek.exe'), '--purge-user-data', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then RaiseException('BeeK data cleanup failed; files and remaining data preserved.');
  end;
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and OwnsRun(Old) then RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK');
 end;
end;

procedure DeinitializeUninstall;
begin if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock); end;
