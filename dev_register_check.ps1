# Non-admin inspection script: prints what install.ps1 will register and checks current status.
$ErrorActionPreference = "Stop"

$guidsFile = Join-Path $PSScriptRoot "tsf\src\guids.rs"
$content = Get-Content $guidsFile -Raw -Encoding UTF8
$clsid   = ([regex]::Match($content, 'CLSID_STR:\s*&str\s*=\s*"([^"]+)"')).Groups[1].Value
$profile = ([regex]::Match($content, 'PROFILE_STR:\s*&str\s*=\s*"([^"]+)"')).Groups[1].Value
$name    = ([regex]::Match($content, 'DISPLAY_NAME:\s*&str\s*=\s*"([^"]+)"')).Groups[1].Value
$tipId   = "0804:$clsid$profile"

Write-Host "================================================================" -ForegroundColor Cyan
Write-Host "  HEYU TSF Registration & Environment Check (Non-Admin Dry Run)" -ForegroundColor Cyan
Write-Host "================================================================" -ForegroundColor Cyan
Write-Host "  Display Name     : $name"
Write-Host "  TSF CLSID        : $clsid"
Write-Host "  TSF Profile GUID : $profile"
Write-Host "  Taskbar TIP ID   : $tipId"
Write-Host "  Target Directory : C:\Program Files\HEYU"
Write-Host ""

foreach ($f in @("heyu-ime.exe", "heyu_tsf.dll", "heyu_tsf_x86.dll", "assets\heyu.ico")) {
    $p = Join-Path $PSScriptRoot $f
    $ok = Test-Path $p
    $len = if ($ok) { (Get-Item $p).Length } else { 0 }
    Write-Host ("  Binary Check     : {0,-20} Exists={1} ({2:N0} bytes)" -f $f, $ok, $len)
}

$reg64 = Test-Path "HKLM:\Software\Classes\CLSID\$clsid\InprocServer32"
$reg32 = Test-Path "HKLM:\Software\WOW6432Node\Classes\CLSID\$clsid\InprocServer32"
$auto  = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "HeyuIME" -ErrorAction SilentlyContinue).HeyuIME

Write-Host ""
Write-Host "  Currently Registered (x64 COM) : $reg64"
Write-Host "  Currently Registered (x86 COM) : $reg32"
Write-Host "  Current HKCU Autostart         : $(if ($auto) { $auto } else { '(not set yet)' })"
Write-Host "================================================================" -ForegroundColor Cyan
