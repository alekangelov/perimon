$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$exePath = Join-Path $scriptDir "target\release\perimon.exe"
$icoPath = Join-Path $scriptDir "app_icon.ico"

$wsh = New-Object -ComObject WScript.Shell

# 1. Clean up any obsolete shortcuts / registry keys from earlier iterations
$startupFolder = [Environment]::GetFolderPath("Startup")
$oldStartup1 = Join-Path $startupFolder "Peripheral Battery Monitor.lnk"
$oldStartup2 = Join-Path $startupFolder "Peripheral Monitor.lnk"
if (Test-Path $oldStartup1) { Remove-Item $oldStartup1 -Force }
if (Test-Path $oldStartup2) { Remove-Item $oldStartup2 -Force }

# Remove old registry Run key if set (to avoid duplicate startup instances)
$regKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Remove-ItemProperty -Path $regKey -Name "PeripheralBatteryMonitor" -ErrorAction SilentlyContinue
Remove-ItemProperty -Path $regKey -Name "perimon" -ErrorAction SilentlyContinue

# 2. Configure Windows Startup Folder (runs on boot minimized to system tray)
$startupShortcutPath = Join-Path $startupFolder "perimon.lnk"
$scStartup = $wsh.CreateShortcut($startupShortcutPath)
$scStartup.TargetPath = $exePath
$scStartup.Arguments = "--minimized"
$scStartup.WorkingDirectory = $scriptDir
$scStartup.Description = "perimon - Peripheral Battery Monitor (Tray Auto-Start)"
$scStartup.IconLocation = "$icoPath,0"
$scStartup.Save()

# 3. Create Desktop Shortcut
$desktop = [Environment]::GetFolderPath("Desktop")
if (-not $desktop) {
    $desktop = Join-Path $env:USERPROFILE "Desktop"
}
$desktopShortcutPath = Join-Path $desktop "perimon.lnk"

# Clean up old desktop shortcuts
$oldDesktopShortcuts = @(
    (Join-Path $desktop "Peripheral Batteries (Rust).lnk"),
    (Join-Path $desktop "Peripheral Monitor.lnk"),
    (Join-Path $desktop "Peripheral Battery Monitor.lnk")
)
foreach ($old in $oldDesktopShortcuts) {
    if (Test-Path $old) { Remove-Item $old -Force }
}

$scDesktop = $wsh.CreateShortcut($desktopShortcutPath)
$scDesktop.TargetPath = $exePath
$scDesktop.WorkingDirectory = $scriptDir
$scDesktop.Description = "perimon - Peripheral Battery Monitor"
$scDesktop.IconLocation = "$icoPath,0"
$scDesktop.Save()

Write-Host "[+] Startup shortcut created: $startupShortcutPath" -ForegroundColor Green
Write-Host "[+] Desktop shortcut created: $desktopShortcutPath" -ForegroundColor Green
Write-Host "[+] perimon configured to run on boot minimized to tray!" -ForegroundColor Cyan
