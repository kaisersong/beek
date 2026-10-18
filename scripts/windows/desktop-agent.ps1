# Test-only desktop driver. Run as an Interactive scheduled task, never ship it.
param([Parameter(Mandatory=$true)][string]$Workspace,[string]$ProjectDirectory='', [ValidateRange(1,720)][int]$Minutes=360)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class BeeKDesktop {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct Mouse { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
 [StructLayout(LayoutKind.Sequential)] public struct Key { public ushort Vk,Scan; public uint Flags,Time; public UIntPtr Extra; }
 [StructLayout(LayoutKind.Explicit)] public struct Union { [FieldOffset(0)] public Mouse M; [FieldOffset(0)] public Key K; }
 [StructLayout(LayoutKind.Sequential)] public struct Input { public uint Type; public Union U; }
 [StructLayout(LayoutKind.Sequential)] public struct Gui { public uint Size,Flags; public IntPtr Active,Focus,Capture,MenuOwner,MoveSize,Caret; public Rect CaretRect; }
 public class WindowInfo { public long Hwnd; public uint Pid; public string Title,Class; public Rect Bounds; public uint Dpi; public long ExStyle; }
 public delegate bool Callback(IntPtr h,IntPtr p);
 [DllImport("kernel32.dll")] public static extern bool GetExitCodeProcess(IntPtr handle,out uint code);
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback c,IntPtr p);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern uint SendInput(uint n,Input[] inputs,int size);
 [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint id,ref Gui info);
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
 [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id,uint flags);
 [DllImport("user32.dll")] static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
 [DllImport("user32.dll")] static extern bool SwitchDesktop(IntPtr desktop);
 [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
 public static bool Unlocked() { IntPtr d=OpenInputDesktop(0,false,0x101); if(d==IntPtr.Zero)return false; bool ok=SwitchDesktop(d); CloseDesktop(d); return ok; }
 public static WindowInfo[] Windows() {
  var all=new List<WindowInfo>();
  EnumWindows(delegate(IntPtr h,IntPtr p){ if(!IsWindowVisible(h))return true; uint pid;GetWindowThreadProcessId(h,out pid);var t=new StringBuilder(1024);var c=new StringBuilder(256);GetWindowText(h,t,t.Capacity);GetClassName(h,c,c.Capacity);Rect r;GetWindowRect(h,out r);all.Add(new WindowInfo{Hwnd=h.ToInt64(),Pid=pid,Title=t.ToString(),Class=c.ToString(),Bounds=r,Dpi=GetDpiForWindow(h),ExStyle=GetWindowLongPtr(h,-20).ToInt64()});return true; },IntPtr.Zero);
  return all.ToArray();
 }
 public static bool Activate(IntPtr h){ShowWindowAsync(h,9);return SetForegroundWindow(h);}
 public static Gui Focus() { uint pid;uint thread=GetWindowThreadProcessId(GetForegroundWindow(),out pid);Gui g=new Gui();g.Size=(uint)Marshal.SizeOf(typeof(Gui));GetGUIThreadInfo(thread,ref g);return g; }
 static void Emit(Input i) { if(SendInput(1,new Input[]{i},Marshal.SizeOf(typeof(Input)))!=1)throw new InvalidOperationException("SendInput rejected"); }
 static void VKey(ushort vk,bool up){Input i=new Input();i.Type=1;i.U.K.Vk=vk;i.U.K.Flags=up?2u:0u;Emit(i);}
 public static void Chord(int[] codes){try{foreach(int c in codes)VKey((ushort)c,false);}finally{for(int i=codes.Length-1;i>=0;i--)VKey((ushort)codes[i],true);}}
 public static void Text(string text){foreach(char c in text){Input i=new Input();i.Type=1;i.U.K.Scan=c;i.U.K.Flags=4;Emit(i);i.U.K.Flags=6;Emit(i);}}
 public static void Click(int x,int y){ClickHeld(x,y,0);}
 public static void ClickHeld(int x,int y,int milliseconds){if(milliseconds<0||milliseconds>2000)throw new ArgumentOutOfRangeException();SetCursorPos(x,y);Input i=new Input();i.Type=0;i.U.M.Flags=2;Emit(i);try{if(milliseconds>0)System.Threading.Thread.Sleep(milliseconds);}finally{i.U.M.Flags=4;Emit(i);}}
 public static void DoubleClick(int x,int y){Click(x,y);System.Threading.Thread.Sleep(65);Click(x,y);}
 public static void Drag(int x,int y,int endX,int endY){DragTimed(x,y,endX,endY,144,0);}
 public static void DragTimed(int x,int y,int endX,int endY,int duration,int pause){if(duration<12||duration>5000||pause<0||pause>2000)throw new ArgumentOutOfRangeException();SetCursorPos(x,y);Input i=new Input();i.Type=0;i.U.M.Flags=2;Emit(i);try{for(int step=1;step<=12;step++){SetCursorPos(x+(endX-x)*step/12,y+(endY-y)*step/12);System.Threading.Thread.Sleep(duration/12);if(step==6&&pause>0)System.Threading.Thread.Sleep(pause);}}finally{i.U.M.Flags=4;Emit(i);}}
 [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
 [DllImport("user32.dll")] static extern bool CloseClipboard();
 static int holding;
 public static bool HoldClipboard(int milliseconds){if(milliseconds<100||milliseconds>2500)throw new ArgumentOutOfRangeException();if(System.Threading.Interlocked.CompareExchange(ref holding,1,0)!=0)return false;bool opened=false;var ready=new System.Threading.ManualResetEventSlim();var t=new System.Threading.Thread(()=>{try{opened=OpenClipboard(IntPtr.Zero);ready.Set();if(opened)System.Threading.Thread.Sleep(milliseconds);}finally{if(opened)CloseClipboard();System.Threading.Interlocked.Exchange(ref holding,0);}});t.IsBackground=true;t.Start();if(!ready.Wait(1000))return false;return opened;}
 public static void RightClick(int x,int y){SetCursorPos(x,y);Input i=new Input();i.Type=0;i.U.M.Flags=8;Emit(i);i.U.M.Flags=16;Emit(i);}
}
'@
$null=[BeeKDesktop]::SetProcessDpiAwarenessContext([IntPtr](-4))
$Workspace=(Resolve-Path $Workspace).Path
if(-not $ProjectDirectory){$ProjectDirectory=if(Test-Path (Join-Path $Workspace 'Cargo.toml')){$Workspace}else{Join-Path $Workspace 'beek-windows-g0-source'}}
$ProjectDirectory=(Resolve-Path $ProjectDirectory).Path
$control=Join-Path $Workspace 'remote-evidence\desktop'
$inbox=Join-Path $control 'inbox';$outbox=Join-Path $control 'outbox';$images=Join-Path $control 'images'
foreach($p in @($inbox,$outbox,$images)){New-Item -ItemType Directory -Path $p -Force | Out-Null}
$initialForeground=[BeeKDesktop]::GetForegroundWindow()
$ownedPids=New-Object 'Collections.Generic.HashSet[int]'
$initialWindows=New-Object 'Collections.Generic.HashSet[long]'
foreach($window in [BeeKDesktop]::Windows()){$null=$initialWindows.Add($window.Hwnd)}
$savedClipboard=$null;$clipboardTouched=$false
function Save-Json($Object,[string]$Path){[IO.File]::WriteAllText($Path,($Object|ConvertTo-Json -Depth 15),(New-Object Text.UTF8Encoding($false)))}
function Window-For([long]$Hwnd){
 $window=@([BeeKDesktop]::Windows()|Where-Object {$_.Hwnd -eq $Hwnd})
 if($window.Count -ne 1){throw 'target window missing'}
 $w=$window[0]
 $process=Get-Process -Id $w.Pid -ErrorAction Stop
 $allowed=($ownedPids.Contains([int]$w.Pid) -and $process.ProcessName -in @('beek','beek-windows-probe')) -or ($process.ProcessName -in @('Notepad','notepad','msedge','chrome') -and $w.Title -match 'beek-g0-public-fixture|BeeK public selection fixture')
 if(-not $allowed){throw 'target is not a BeeK test window or public fixture'}
 return $w
}
function Require-Foreground([long]$Hwnd){$null=Window-For $Hwnd;if([BeeKDesktop]::GetForegroundWindow().ToInt64() -ne $Hwnd){throw 'target is not foreground'}}
function Remember-Clipboard {if(-not $script:clipboardTouched){$script:savedClipboard=[Windows.Forms.Clipboard]::GetDataObject();$script:clipboardTouched=$true}}
function Find-TrayIcon {
 $probe=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,'BeeK G0 probe')
 $product=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,'BeeK')
 $condition=New-Object Windows.Automation.OrCondition($probe,$product)
 foreach($window in @([BeeKDesktop]::Windows() | Where-Object {$_.Class -match 'Shell_TrayWnd|Overflow|XamlExplorerHost'})) {
  $root=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$window.Hwnd)
  $icons=$root.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)
  foreach($icon in $icons){
   if(-not $icon.Current.IsOffscreen -and $icon.Current.ClassName -eq 'SystemTray.NormalButton'){return $icon}
  }
 }
 return $null
}
$ready=[ordered]@{pid=$PID;session_id=(Get-Process -Id $PID).SessionId;interactive=[Environment]::UserInteractive;unlocked=[BeeKDesktop]::Unlocked();screens=@([Windows.Forms.Screen]::AllScreens|ForEach-Object {@{device=$_.DeviceName;primary=$_.Primary;x=$_.Bounds.X;y=$_.Bounds.Y;width=$_.Bounds.Width;height=$_.Bounds.Height}})}
Save-Json $ready (Join-Path $control 'ready.json')
$end=(Get-Date).AddMinutes($Minutes);$exitRequested=$false
try {
 while((Get-Date) -lt $end -and -not $exitRequested){
  foreach($file in @(Get-ChildItem $inbox -Filter '*.json'|Sort-Object Name)){
   $request=$null
   try {
    if($file.Length -gt 65536){throw 'request exceeds 64KiB'}
    $request=Get-Content $file.FullName -Raw -Encoding UTF8|ConvertFrom-Json
    if($request.id -notmatch '^[a-zA-Z0-9-]{1,80}$'){throw 'invalid request ID'}
    $value=$null
    switch($request.action){
     'state' {$value=@{session_id=(Get-Process -Id $PID).SessionId;unlocked=[BeeKDesktop]::Unlocked();foreground=[BeeKDesktop]::GetForegroundWindow().ToInt64();focus=[BeeKDesktop]::Focus();windows=@([BeeKDesktop]::Windows()|Where-Object {($ownedPids.Contains([int]$_.Pid) -and $_.Class -ne 'Chrome_WidgetWin_1') -or $_.Title -match 'beek-g0-public-fixture|BeeK public selection fixture'})}}
     'package-install' {
      $path=[IO.Path]::GetFullPath([string]$request.path);$target=[IO.Path]::GetFullPath([string]$request.directory)
      if(-not $path.StartsWith($Workspace+'\',[StringComparison]::OrdinalIgnoreCase) -or $path -notmatch 'BeeK-v[0-9.]+-windows-x64-setup\.exe$' -or -not $target.StartsWith($Workspace+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'installer must stay in this test workspace'}
      $log=Join-Path $outbox ($request.id+'.install.log');$process=[Diagnostics.Process]::new();$process.StartInfo.FileName=$path;$process.StartInfo.Arguments='/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /DIR="'+$target+'" /LOG="'+$log+'"';$process.StartInfo.UseShellExecute=$false;$null=$process.Start()
      if(-not $process.WaitForExit(30000)){throw ('installer still running; inspect owned PID '+$process.Id)}
      $value=@{exit_code=$process.ExitCode;log=$log;directory=$target;session_id=(Get-Process -Id $PID).SessionId};$process.Dispose()
     }
     'package-uninstall' {
      $path=[IO.Path]::GetFullPath([string]$request.path)
      if(-not $path.StartsWith($Workspace+'\',[StringComparison]::OrdinalIgnoreCase) -or $path -notmatch '\\unins[0-9]{3}\.exe$'){throw 'only test workspace uninstaller allowed'}
      $log=Join-Path $outbox ($request.id+'.uninstall.log');$process=[Diagnostics.Process]::new();$process.StartInfo.FileName=$path;$process.StartInfo.Arguments='/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="'+$log+'"';$process.StartInfo.UseShellExecute=$false;$null=$process.Start()
      if(-not $process.WaitForExit(30000)){throw ('uninstaller still running; inspect owned PID '+$process.Id)}
      $value=@{exit_code=$process.ExitCode;log=$log;session_id=(Get-Process -Id $PID).SessionId};$process.Dispose()
     }
     'measure-idle' {
      $script=Join-Path $ProjectDirectory 'scripts\windows\measure-idle.ps1';$name=if($request.PSObject.Properties.Name -contains 'name'){[string]$request.name}else{'idle-product.csv'};if($name -notmatch '^idle-product[-a-zA-Z0-9]*\.csv$'){throw 'invalid idle evidence name'};$output=Join-Path $control $name;$stdout=Join-Path $control 'idle-product.stdout.txt';$stderr=Join-Path $control 'idle-product.stderr.txt'
      if(Test-Path $output){throw 'idle evidence already exists; preserve it'}
      $process=Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList @('-NoProfile','-File',('"'+$script+'"'),'-OutputFile',('"'+$output+'"'),'-Seconds','300') -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
      $value=@{pid=$process.Id;output=$output;session_id=(Get-Process -Id $PID).SessionId}
     }
     'rust-tests' {
      $path=[IO.Path]::GetFullPath([string]$request.path)
      if(-not $path.StartsWith($Workspace,[StringComparison]::OrdinalIgnoreCase) -or $path -notmatch '\\debug\\deps\\beek-[0-9a-f]+\.exe$'){throw 'only compiled workspace BeeK unit tests allowed'}
      $stdout=Join-Path $outbox ($request.id+'.stdout.txt');$stderr=Join-Path $outbox ($request.id+'.stderr.txt')
      $process=Start-Process -FilePath $path -ArgumentList @('--test-threads=1','--include-ignored') -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
      $handle=$process.Handle
      if(-not $process.WaitForExit(15000)){$process.Kill();throw 'unit test runner exceeded 15 seconds'}
      $exitCode=[uint32]0;if(-not [BeeKDesktop]::GetExitCodeProcess($handle,[ref]$exitCode)){throw 'cannot read test exit code'}
      $value=@{exit_code=$exitCode;stdout=[IO.File]::ReadAllText($stdout);stderr=[IO.File]::ReadAllText($stderr);session_id=(Get-Process -Id $PID).SessionId}
     }
     'adopt' {$p=Get-Process -Id ([int]$request.pid);if($p.ProcessName -notin @('beek-windows-probe','beek') -or -not $p.Path.StartsWith($Workspace,[StringComparison]::OrdinalIgnoreCase) -or $p.SessionId -ne (Get-Process -Id $PID).SessionId){throw 'only this workspace BeeK process can be adopted'};$null=$ownedPids.Add($p.Id);$value=$true}
     'launch' {
      $path=[IO.Path]::GetFullPath([string]$request.path);$name=[IO.Path]::GetFileName($path).ToLowerInvariant()
      $allowed=($name -in @('beek-windows-probe.exe','beek.exe','beek-windows-harness.exe') -and $path.StartsWith($Workspace,[StringComparison]::OrdinalIgnoreCase)) -or ($name -eq 'notepad.exe' -and $path.StartsWith($env:WINDIR,[StringComparison]::OrdinalIgnoreCase)) -or ($name -in @('msedge.exe','chrome.exe') -and ($path.StartsWith($env:ProgramFiles,[StringComparison]::OrdinalIgnoreCase) -or $path.StartsWith(${env:ProgramFiles(x86)},[StringComparison]::OrdinalIgnoreCase)))
      if(-not $allowed){throw 'launch target not allowed'}
      $launchArgs=@($request.arguments|ForEach-Object {'"'+([string]$_).Replace('"','')+'"'})
      $process=if($launchArgs.Count){Start-Process -FilePath $path -ArgumentList $launchArgs -PassThru}else{Start-Process -FilePath $path -PassThru}
      $null=$ownedPids.Add($process.Id);$value=@{pid=$process.Id}
     }
     'activate' {$w=Window-For $request.hwnd;$value=[BeeKDesktop]::Activate([IntPtr]$w.Hwnd)}
     'move' {$w=Window-For $request.hwnd;$value=[BeeKDesktop]::SetWindowPos([IntPtr]$w.Hwnd,[IntPtr]::Zero,[int]$request.x,[int]$request.y,[int]$request.width,[int]$request.height,0x14)}
     'click' {$w=Window-For $request.hwnd;$x=$w.Bounds.Left+[int]$request.x;$y=$w.Bounds.Top+[int]$request.y;if($x -lt $w.Bounds.Left -or $x -ge $w.Bounds.Right -or $y -lt $w.Bounds.Top -or $y -ge $w.Bounds.Bottom){throw 'click outside target'};$hold=if($request.PSObject.Properties.Name -contains 'hold_ms'){[int]$request.hold_ms}else{0};[BeeKDesktop]::ClickHeld($x,$y,$hold);$value=$true}
     'selection-gesture' {
      Require-Foreground $request.hwnd;$w=Window-For $request.hwnd
      if($w.Title -notmatch 'beek-g0-public-fixture|BeeK public selection fixture'){throw 'selection gesture requires a public fixture'}
      $x=$w.Bounds.Left+[int]$request.x;$y=$w.Bounds.Top+[int]$request.y
      if($x -lt $w.Bounds.Left -or $x -ge $w.Bounds.Right -or $y -lt $w.Bounds.Top -or $y -ge $w.Bounds.Bottom){throw 'gesture outside target'}
      if($request.mode -eq 'double'){[BeeKDesktop]::DoubleClick($x,$y)}
      elseif($request.mode -eq 'drag'){$endX=$w.Bounds.Left+[int]$request.end_x;$endY=$w.Bounds.Top+[int]$request.end_y;if($endX -lt $w.Bounds.Left -or $endX -ge $w.Bounds.Right -or $endY -lt $w.Bounds.Top -or $endY -ge $w.Bounds.Bottom){throw 'gesture endpoint outside target'};$duration=if($request.PSObject.Properties.Name -contains 'duration_ms'){[int]$request.duration_ms}else{144};$pause=if($request.PSObject.Properties.Name -contains 'pause_ms'){[int]$request.pause_ms}else{0};[BeeKDesktop]::DragTimed($x,$y,$endX,$endY,$duration,$pause)}
      else{throw 'unsupported selection gesture'}
      $value=$true
     }
     'keys' {Require-Foreground $request.hwnd;[BeeKDesktop]::Chord([int[]]$request.codes);$value=$true}
     'key-sequence' {Require-Foreground $request.hwnd;if($request.chords.Count -gt 20){throw 'too many chords'};foreach($chord in $request.chords){[BeeKDesktop]::Chord([int[]]$chord)};$value=$true}
     'text' {Require-Foreground $request.hwnd;if($request.text.Length -gt 16384){throw 'text too long'};[BeeKDesktop]::Text([string]$request.text);$value=$true}
     'clipboard-hold' {Require-Foreground $request.hwnd;$value=[BeeKDesktop]::HoldClipboard([int]$request.milliseconds)}
     'clipboard-set' {Require-Foreground $request.hwnd;Remember-Clipboard;[Windows.Forms.Clipboard]::SetText([string]$request.text);$value=$true}
     'clipboard-compare' {Require-Foreground $request.hwnd;$text=[Windows.Forms.Clipboard]::GetText();$value=@{length=$text.Length;matches=($text -ceq [string]$request.expected)}}
     'keyboard' {Require-Foreground $request.hwnd;$hkl=[BeeKDesktop]::LoadKeyboardLayout([string]$request.layout,0);$null=[BeeKDesktop]::PostMessage([IntPtr]$request.hwnd,0x50,[IntPtr]::Zero,$hkl);$value=@{requested=$hkl.ToInt64()}}
     'tray' {
      $icon=Find-TrayIcon
      if($null -eq $icon){
       $tray=@([BeeKDesktop]::Windows()|Where-Object {$_.Class -eq 'Shell_TrayWnd'})
       if($tray.Count -ne 1){throw 'taskbar missing'}
       $root=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$tray[0].Hwnd)
       $buttons=$root.FindAll([Windows.Automation.TreeScope]::Descendants,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button)))
       foreach($button in $buttons){if($button.Current.AutomationId -match 'Overflow|Chevron' -or $button.Current.Name -match 'hidden|System tray overflow' -or $button.Current.Name.Contains([string][char]0x9690)){$r=$button.Current.BoundingRectangle;[BeeKDesktop]::Click([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2));break}}
       Start-Sleep -Milliseconds 400
       $icon=Find-TrayIcon
      }
      if($null -eq $icon){throw 'BeeK notification icon not found in taskbar/overflow'}
      $r=$icon.Current.BoundingRectangle
      if($request.right){[BeeKDesktop]::RightClick([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}else{[BeeKDesktop]::Click([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
      $value=@{icon='BeeK G0 probe';x=$r.X;y=$r.Y;right=$request.right}
     }
     'tray-debug' {
      $value=@(foreach($window in @([BeeKDesktop]::Windows()|Where-Object {$_.Class -match 'Shell_TrayWnd|Overflow|XamlExplorerHost'})){
       $root=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$window.Hwnd)
       $elements=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
       foreach($item in $elements){$name=$item.Current.Name;if($name -match 'BeeK|hidden|overflow' -or $name.Contains([string][char]0x9690) -or $item.Current.AutomationId -match 'Overflow|Chevron'){@{name=$name;id=$item.Current.AutomationId;class=$item.Current.ClassName;offscreen=$item.Current.IsOffscreen;bounds=$item.Current.BoundingRectangle}}}
      })
     }
     'menu' {
      if($request.label -notin @('Open input / IME probe','Show non-activating popup','Close all probe windows','Exit probe','Settings','Read clipboard...','Restore selection detection','About / updates','Restart BeeK','Quit BeeK')){throw 'unsupported test menu action'}
      $menu=@([BeeKDesktop]::Windows()|Where-Object {$_.Class -eq '#32768' -and $ownedPids.Contains([int]$_.Pid)})
      if($menu.Count -ne 1){throw 'probe menu missing'}
      $root=[Windows.Automation.AutomationElement]::FromHandle([IntPtr]$menu[0].Hwnd)
      $item=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,[string]$request.label)))
      if($null -ne $item){$r=$item.Current.BoundingRectangle;[BeeKDesktop]::Click([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
      else {
       # Classic Win32 menus can expose only a root UIA element. Select a
       # visible, known row using its actual menu rectangle, not a message shim.
       $labels=if($request.label -in @('Settings','Read clipboard...','Restore selection detection','About / updates','Restart BeeK','Quit BeeK')){@('Settings','Read clipboard...','Restore selection detection','About / updates','Restart BeeK','Quit BeeK')}else{@('Open input / IME probe','Show non-activating popup','Close all probe windows','Exit probe')}
       $index=[array]::IndexOf($labels,[string]$request.label);$r=$menu[0].Bounds
       $row=($r.Bottom-$r.Top-6)/$labels.Count
       [BeeKDesktop]::Click($r.Left+120,[int]($r.Top+3+($index+0.5)*$row))
      }
      $value=$true
     }
     'capture' {
      $w=Window-For $request.hwnd;$r=$w.Bounds;$bitmap=New-Object Drawing.Bitmap(($r.Right-$r.Left),($r.Bottom-$r.Top))
      $graphics=[Drawing.Graphics]::FromImage($bitmap)
      try {$graphics.CopyFromScreen($r.Left,$r.Top,0,0,$bitmap.Size);$path=Join-Path $images ($request.id+'.png');$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png);$value=@{path=$path;bounds=$r}} finally {$graphics.Dispose();$bitmap.Dispose()}
     }
     'close' {$w=Window-For $request.hwnd;if($initialWindows.Contains($w.Hwnd)){throw 'will not close a pre-existing window'};$value=[BeeKDesktop]::PostMessage([IntPtr]$w.Hwnd,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
     'explorer-restart' {
      $session=(Get-Process -Id $PID).SessionId
      if($session -eq 0 -or -not [BeeKDesktop]::Unlocked()){throw 'requires the unlocked interactive test session'}
      $shell=New-Object -ComObject Shell.Application
      if(@($shell.Windows()|Where-Object {$_.FullName -match '\\explorer.exe$'}).Count){throw 'Explorer has open folder windows; do not disturb them'}
      $explorers=@(Get-Process explorer|Where-Object {$_.SessionId -eq $session})
      if($explorers.Count -ne 1){throw 'requires one Explorer shell in the current session'}
      $visible=@([BeeKDesktop]::Windows()|Where-Object {$_.Pid -eq $explorers[0].Id})
      $unexpected=@($visible|Where-Object {$_.Class -notmatch '^(Shell_TrayWnd|Shell_SecondaryTrayWnd|Progman|WorkerW|NotifyIconOverflowWindow|TopLevelWindowForOverflowXamlIsland|XamlExplorerHostIslandWindow)$'})
      if($unexpected.Count){throw 'Explorer has a visible non-shell window; leave shell restart to the tester'}
      $before=$explorers[0].Id;Stop-Process -Id $before -Force
      Start-Sleep -Milliseconds 400
      $new=Start-Process (Join-Path $env:WINDIR 'explorer.exe') -PassThru
      $value=@{old_pid=$before;launcher_pid=$new.Id;session_id=$session;folder_windows=0}
     }
     'quit' {$exitRequested=$true;$value=$true}
     default {throw 'unknown test action'}
    }
    $response=@{ok=$true;id=$request.id;action=$request.action;value=$value;utc=[DateTime]::UtcNow.ToString('o')}
   } catch {$response=@{ok=$false;error=$_.ToString();position=$_.InvocationInfo.PositionMessage;utc=[DateTime]::UtcNow.ToString('o')}}
   $result=Join-Path $outbox ($file.BaseName+'.json');Save-Json $response $result
   Remove-Item $file.FullName
  }
  Start-Sleep -Milliseconds 100
 }
} finally {
 if($clipboardTouched -and $null -ne $savedClipboard){try{[Windows.Forms.Clipboard]::SetDataObject($savedClipboard,$true)}catch{}}
 $null=[BeeKDesktop]::SetForegroundWindow($initialForeground)
 Save-Json @{stopped=$true;utc=[DateTime]::UtcNow.ToString('o')} (Join-Path $control 'stopped.json')
}
