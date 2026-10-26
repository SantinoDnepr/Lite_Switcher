param([string]$Exe)
# End-to-end check of the automatic literary mode in a WinForms TextBox (isolated APPDATA, test instance).
# Run it only when nobody works at the computer: it types into a window that takes the focus.
# usage: pwsh -File tools\e2e\auto-literary.ps1 -Exe target\debug\lite-switcher.exe
# Every scenario prints the text of the box and the layout of the window afterwards.
[Console]::OutputEncoding = [Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class K5 {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static void Force(IntPtr h) { uint pid; uint fg = GetWindowThreadProcessId(GetForegroundWindow(), out pid); uint me = GetCurrentThreadId(); AttachThreadInput(me, fg, true); SetForegroundWindow(h); AttachThreadInput(me, fg, false); }
}
"@
$ad = Join-Path $env:TEMP "ls-e2e-auto"
Remove-Item $ad -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$ad\LiteSwitcher" | Out-Null
"[common]`nmode = auto_literary`n[hotkeys]`nconvert_word = ScrollLock`nconvert_selection =`ninvert_case =`n[troubleshooting]`nlog_enabled = true`nlog_level = debug`n" | Set-Content "$ad\LiteSwitcher\config.ini"
$env:APPDATA = $ad; $env:LITE_SWITCHER_TEST_INSTANCE = "1"
$daemon = Start-Process $Exe -PassThru
$form = New-Object Windows.Forms.Form
$tb = New-Object Windows.Forms.TextBox
$tb.Multiline = $true; $tb.Dock = 'Fill'; $form.Controls.Add($tb); $form.Width = 420; $form.Height = 200; $form.TopMost = $true
# key table: char -> vk, scancode, shift
$scan = @{ q=0x10;w=0x11;e=0x12;r=0x13;t=0x14;y=0x15;u=0x16;i=0x17;o=0x18;p=0x19;a=0x1E;s=0x1F;d=0x20;f=0x21;g=0x22;h=0x23;j=0x24;k=0x25;l=0x26;z=0x2C;x=0x2D;c=0x2E;v=0x2F;b=0x30;n=0x31;m=0x32 }
function Pump($ms) { $end = [DateTime]::Now.AddMilliseconds($ms); while ([DateTime]::Now -lt $end) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 10 } }
$script:lost = $false
function KeyDownUp($vk, $sc, [bool]$shift = $false) {
  if ($script:lost -or [K5]::GetForegroundWindow() -ne $form.Handle) { $script:lost = $true; return }
  if ($shift) { [K5]::keybd_event(0x10, 0x2A, 0, [UIntPtr]::Zero) }
  [K5]::keybd_event($vk, $sc, 0, [UIntPtr]::Zero); [K5]::keybd_event($vk, $sc, 2, [UIntPtr]::Zero)
  if ($shift) { [K5]::keybd_event(0x10, 0x2A, 2, [UIntPtr]::Zero) }
  Pump 45
}
function TypeText($text) { foreach ($c in $text.ToCharArray()) { $l = [string]$c; if ($l -eq ' ') { KeyDownUp 0x20 0x39 } elseif ($l -eq "'") { KeyDownUp 0xDE 0x28 } else { KeyDownUp ([int][char]$l.ToUpper()) $scan[$l] } } }
function Enter { KeyDownUp 0x0D 0x1C }
function Back { KeyDownUp 0x08 0x0E }
function Layout($name) { $l = [Windows.Forms.InputLanguage]::InstalledInputLanguages | Where-Object { $_.Culture.Name -eq $name } | Select-Object -First 1; [Windows.Forms.InputLanguage]::CurrentInputLanguage = $l; Pump 150 }
function Show($label) { $codes = ($tb.Text.ToCharArray() | ForEach-Object { if ($_ -eq "`r") {'\r'} elseif ($_ -eq "`n") {'\n'} else {[string]$_} }) -join ''; "{0,-34}: [{1}] layout={2}" -f $label, $codes, [Windows.Forms.InputLanguage]::CurrentInputLanguage.Culture.Name }
function Reset($lang) { $tb.Text = ''; Layout $lang; Pump 200 }
try {
  $form.Show(); [K5]::keybd_event(0x12,0x38,0,[UIntPtr]::Zero); [K5]::keybd_event(0x12,0x38,2,[UIntPtr]::Zero); $form.Activate(); [K5]::Force($form.Handle)
  $tb.Focus() | Out-Null
  Pump 2500   # the daemon opens the packs in the background
  function Scenario($label, $lang, [scriptblock]$action) {
    for ($try = 1; $try -le 5; $try++) {
      $script:lost = $false
      $tb.Text = ''; [K5]::Force($form.Handle); $tb.Focus() | Out-Null; Layout $lang; Pump 250
      if ([K5]::GetForegroundWindow() -ne $form.Handle) { Start-Sleep -Milliseconds 700; continue }
      & $action
      Pump 450
      if (-not $script:lost) { Show $label; return }
    }
    "{0,-34}: SKIPPED (the test window kept losing the focus)" -f $label
  }
  "step: foreground ok"
  Scenario 'a. en: ghbdtn + space' 'en-US' { TypeText 'ghbdtn ' }
  Scenario 'b. a + hotkey twice' 'en-US' { TypeText 'ghbdtn '; Pump 300; KeyDownUp 0x91 0x46; Pump 400; KeyDownUp 0x91 0x46 }
  Scenario 'c. ru layout: hello (cyr. shown)' 'ru-RU' { TypeText 'hello ' }
  Scenario 'd. en: hello + space (keep)' 'en-US' { TypeText 'hello ' }
  Scenario 'e. en: ghbdtn + Enter' 'en-US' { TypeText 'ghbdtn'; Enter }
  Scenario 'f. en: ghbdtn + Shift+/ (? -> ,)' 'en-US' { TypeText 'ghbdtn'; KeyDownUp 0x35 0x35 $true }
  Scenario 'g. backspace edits (no auto)' 'en-US' { TypeText 'ghbdtx'; Back; TypeText 'n ' }
  Scenario 'h. quoted hello (keep)' 'en-US' { TypeText "'hello' " }
  Scenario 'i. two words in a row' 'en-US' { TypeText 'ghbdtn ghbdtn ' }
  Scenario 'j. arrows in the word (no auto)' 'en-US' { TypeText 'ghb'; KeyDownUp 0x25 0x4B; KeyDownUp 0x27 0x4D; TypeText 'dtn ' }
  Scenario 'k. fast typing, next word at once' 'en-US' { TypeText 'ghbdtn vfvf' }
} finally {
  $form.Close()
  Stop-Process -Id $daemon.Id -Force -ErrorAction SilentlyContinue
  "--- daemon log ---"
  Get-Content "$ad\LiteSwitcher\lite-switcher.log" -ErrorAction SilentlyContinue | Select-Object -Last 40
}

