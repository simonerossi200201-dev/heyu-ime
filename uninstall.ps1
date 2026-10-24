param(
    [switch]$ElevatedStep,
    [switch]$PurgeUserData,
    [string]$SourceDir = $PSScriptRoot
)

$ErrorActionPreference = "Continue"

function U([int[]]$codes) { -join ($codes | ForEach-Object { [char]$_ }) }
$NAME_HEYU_IME   = U @(0x9E64,0x7FBD,0x97F3,0x5F62)
$NAME_STUDIO_LNK = (U @(0x9E64,0x7FBD,0x97F3,0x5F62,0x63A7,0x5236,0x53F0)) + ".lnk"
$NAME_START_LNK  = (U @(0x542F,0x52A8,0x9E64,0x7FBD,0x97F3,0x5F62)) + ".lnk"
$OLD_HEYU_IME    = U @(0x9E64,0x7FBD,0x8F93,0x5165,0x6CD5)
$OLD_STUDIO_LNK  = (U @(0x9E64,0x7FBD,0x8F93,0x5165,0x6CD5,0x63A7,0x5236,0x53F0)) + ".lnk"
$OLD_START_LNK   = (U @(0x542F,0x52A8,0x9E64,0x7FBD,0x8F93,0x5165,0x6CD5)) + ".lnk"

$CLSID_STR   = "{C8A71319-6351-406E-8FF3-A229E1FC7663}"
$PROFILE_STR = "{92AF8AF0-4D38-489E-8C63-E263D15DFEE4}"
$TIP_ID      = "0804:$CLSID_STR$PROFILE_STR"
$MS_PINYIN_TIP = "0804:{81D4E9C9-1D3B-41BC-9E6C-4B40BF79E35E}{FA550B04-5AD7-411F-A5AC-CA038EC515D7}"
$INSTALL_DIR = "C:\Program Files\HEYU"

if (-not $SourceDir -or $SourceDir -eq "") {
    $SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
}
$SourceDir = $SourceDir.TrimEnd('\', '/')

function Test-IsAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = New-Object Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-UserCleanup {
    Write-Host "[1/3] Removing TIP from user language list, Autostart & Desktop shortcuts..." -ForegroundColor Cyan
    try {
        $list = Get-WinUserLanguageList
        $changed = $false
        foreach ($lang in $list) {
            if ($lang.InputMethodTips -contains $TIP_ID) {
                if ($lang.LanguageTag -like "zh*" -and ($lang.InputMethodTips -notcontains $MS_PINYIN_TIP)) {
                    $lang.InputMethodTips.Add($MS_PINYIN_TIP)
                }
                $lang.InputMethodTips.Remove($TIP_ID) | Out-Null
                $changed = $true
            }
        }
        if ($changed) {
            Set-WinUserLanguageList $list -Force -WarningAction SilentlyContinue
        }
    } catch {}

    Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "HeyuIME" -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName "HeyuImeLauncher" -Confirm:$false -ErrorAction SilentlyContinue

    $desktop = [Environment]::GetFolderPath("Desktop")
    $startup = [Environment]::GetFolderPath("Startup")
    Remove-Item -Force -LiteralPath (Join-Path $startup "$NAME_HEYU_IME.lnk") -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $desktop $NAME_START_LNK)     -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $desktop $NAME_STUDIO_LNK)    -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $startup "$OLD_HEYU_IME.lnk") -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $desktop $OLD_START_LNK)      -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $desktop $OLD_STUDIO_LNK)     -ErrorAction SilentlyContinue
    Remove-Item -Force -LiteralPath (Join-Path $SourceDir "data\tsf_registered.marker") -ErrorAction SilentlyContinue

    if ($PurgeUserData -and $env:APPDATA) {
        Remove-Item -Recurse -Force -LiteralPath (Join-Path $env:APPDATA "HEYU") -ErrorAction SilentlyContinue
        if ($env:LOCALAPPDATA) {
            Remove-Item -Recurse -Force -LiteralPath (Join-Path $env:LOCALAPPDATA "HEYU") -ErrorAction SilentlyContinue
        }
    } elseif ($env:APPDATA) {
        Remove-Item -Force -LiteralPath (Join-Path $env:APPDATA "HEYU\data\tsf_registered.marker") -ErrorAction SilentlyContinue
    }
}

function Invoke-ElevatedCleanup {
    Write-Host "[2/3] Stopping heyu-ime.exe / heyu-voice.exe and unregistering TSF DLLs..." -ForegroundColor Cyan
    Get-Process -Name @("heyu-ime", "heyu-voice") -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400

    if (Test-Path -LiteralPath "$INSTALL_DIR\heyu_tsf.dll") {
        Start-Process -FilePath "$env:SystemRoot\System32\regsvr32.exe" -ArgumentList "/u /s `"$INSTALL_DIR\heyu_tsf.dll`"" -Wait -NoNewWindow -ErrorAction SilentlyContinue | Out-Null
    }
    if (Test-Path -LiteralPath "$INSTALL_DIR\heyu_tsf_x86.dll") {
        Start-Process -FilePath "$env:SystemRoot\SysWOW64\regsvr32.exe" -ArgumentList "/u /s `"$INSTALL_DIR\heyu_tsf_x86.dll`"" -Wait -NoNewWindow -ErrorAction SilentlyContinue | Out-Null
    }

    Write-Host "[3/3] Removing $INSTALL_DIR ..." -ForegroundColor Cyan
    Remove-Item -Recurse -Force -LiteralPath $INSTALL_DIR -ErrorAction SilentlyContinue
    Write-Host "HEYU uninstalled cleanly. (Note: any open apps that loaded heyu_tsf.dll will release it on restart.)" -ForegroundColor Green
}

if ($ElevatedStep) {
    Invoke-ElevatedCleanup
    exit 0
}

Invoke-UserCleanup
if (Test-IsAdmin) {
    Invoke-ElevatedCleanup
} else {
    $scriptPath = $PSCommandPath
    if (-not $scriptPath) { $scriptPath = Join-Path $SourceDir "uninstall.ps1" }
    $cmd = "& '$($scriptPath.Replace("'", "''"))' -ElevatedStep -SourceDir '$($SourceDir.Replace("'", "''"))'"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cmd))
    Start-Process -FilePath "powershell.exe" -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-EncodedCommand", $encoded) -Verb RunAs -Wait
}
