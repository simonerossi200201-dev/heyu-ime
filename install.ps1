param(
    [switch]$ElevatedStep,
    [switch]$UserStepOnly,
    [string]$SourceDir = $PSScriptRoot,
    [string]$LogFile = ""
)

$ErrorActionPreference = "Stop"

# Pure ASCII script: all non-ASCII strings are built from UTF-16 code points via U()
# so PowerShell 5.1 works identically regardless of BOM or system ANSI codepage.
function U([int[]]$codes) { -join ($codes | ForEach-Object { [char]$_ }) }
$NAME_HEYU_IME   = U @(0x9E64,0x7FBD,0x97F3,0x5F62)
$NAME_STUDIO_LNK = (U @(0x9E64,0x7FBD,0x97F3,0x5F62,0x63A7,0x5236,0x53F0)) + ".lnk"
$NAME_START_LNK  = (U @(0x542F,0x52A8,0x9E64,0x7FBD,0x97F3,0x5F62)) + ".lnk"
$NAME_MANUAL_MD  = (U @(0x9E64,0x7FBD,0x97F3,0x5F62)) + "_" + (U @(0x5B8C,0x5168,0x6307,0x5357)) + ".md"

$CLSID_STR   = "{C8A71319-6351-406E-8FF3-A229E1FC7663}"
$PROFILE_STR = "{92AF8AF0-4D38-489E-8C63-E263D15DFEE4}"
$TIP_ID      = "0804:$CLSID_STR$PROFILE_STR"
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

function Copy-SafeFile([string]$src, [string]$dst) {
    try {
        Copy-Item -Force -LiteralPath $src -Destination $dst -ErrorAction Stop
    } catch {
        if (Test-Path -LiteralPath $dst) {
            $bak = "$dst.old.$([Guid]::NewGuid().ToString('N').Substring(0,8))"
            Move-Item -Force -LiteralPath $dst -Destination $bak -ErrorAction SilentlyContinue
        }
        Copy-Item -Force -LiteralPath $src -Destination $dst -ErrorAction Stop
    }
}

function Invoke-ElevatedStep {
    Write-Host "[1/4] Stopping running heyu-ime.exe / heyu-voice.exe (if any)..." -ForegroundColor Cyan
    Get-Process -Name @("heyu-ime", "heyu-voice") -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400

    # Unregister previous DLLs if already present so files are unlocked
    if (Test-Path -LiteralPath "$INSTALL_DIR\heyu_tsf.dll") {
        Start-Process -FilePath "$env:SystemRoot\System32\regsvr32.exe" -ArgumentList "/u /s `"$INSTALL_DIR\heyu_tsf.dll`"" -Wait -NoNewWindow -ErrorAction SilentlyContinue | Out-Null
    }
    if (Test-Path -LiteralPath "$INSTALL_DIR\heyu_tsf_x86.dll") {
        Start-Process -FilePath "$env:SystemRoot\SysWOW64\regsvr32.exe" -ArgumentList "/u /s `"$INSTALL_DIR\heyu_tsf_x86.dll`"" -Wait -NoNewWindow -ErrorAction SilentlyContinue | Out-Null
    }

    Write-Host "[2/4] Copying binaries, assets, docs & lexicons to $INSTALL_DIR ..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
    New-Item -ItemType Directory -Force -Path "$INSTALL_DIR\data" | Out-Null
    New-Item -ItemType Directory -Force -Path "$INSTALL_DIR\docs" | Out-Null
    New-Item -ItemType Directory -Force -Path "$INSTALL_DIR\tools" | Out-Null
    New-Item -ItemType Directory -Force -Path "$INSTALL_DIR\assets" | Out-Null
    New-Item -ItemType Directory -Force -Path "$INSTALL_DIR\assets\previews" | Out-Null

    # Clean up any old rotated DLL backups best-effort
    Get-ChildItem -LiteralPath $INSTALL_DIR -Filter "*.old.*" -ErrorAction SilentlyContinue | ForEach-Object {
        Remove-Item -Force -LiteralPath $_.FullName -ErrorAction SilentlyContinue
    }

    $exeSrc   = Join-Path $SourceDir "heyu-ime.exe"
    $voiceSrc = Join-Path $SourceDir "heyu-voice.exe"
    $dll64Src = Join-Path $SourceDir "heyu_tsf.dll"
    $dll32Src = Join-Path $SourceDir "heyu_tsf_x86.dll"

    if (!(Test-Path -LiteralPath $exeSrc))   { throw "Missing $exeSrc - run cargo build --release first." }
    if (!(Test-Path -LiteralPath $dll64Src)) { throw "Missing $dll64Src - run cargo build --release -p heyu-tsf first." }
    if (!(Test-Path -LiteralPath $dll32Src)) { throw "Missing $dll32Src - run cargo build --release -p heyu-tsf --target i686-pc-windows-gnullvm first." }

    Copy-SafeFile $exeSrc   "$INSTALL_DIR\heyu-ime.exe"
    if (Test-Path -LiteralPath $voiceSrc) { Copy-SafeFile $voiceSrc "$INSTALL_DIR\heyu-voice.exe" }
    Copy-SafeFile $dll64Src "$INSTALL_DIR\heyu_tsf.dll"
    Copy-SafeFile $dll32Src "$INSTALL_DIR\heyu_tsf_x86.dll"

    foreach ($f in @("heyu_studio.py", "heyu_installer.py", "README.md", $NAME_MANUAL_MD, "uninstall.ps1")) {
        $p = Join-Path $SourceDir $f
        if (Test-Path -LiteralPath $p) {
            Copy-SafeFile $p (Join-Path $INSTALL_DIR $f)
        }
    }

    $docsSrc = Join-Path $SourceDir "docs"
    if (Test-Path -LiteralPath $docsSrc) {
        Get-ChildItem -LiteralPath $docsSrc -File -ErrorAction SilentlyContinue | ForEach-Object {
            Copy-SafeFile $_.FullName (Join-Path "$INSTALL_DIR\docs" $_.Name)
        }
    }

    $fetchPs1 = Join-Path $SourceDir "tools\fetch_voice_model.ps1"
    if (Test-Path -LiteralPath $fetchPs1) {
        Copy-SafeFile $fetchPs1 "$INSTALL_DIR\tools\fetch_voice_model.ps1"
    }

    $dataSrc = Join-Path $SourceDir "data"
    if (Test-Path -LiteralPath $dataSrc) {
        Get-ChildItem -LiteralPath $dataSrc -File | ForEach-Object {
            Copy-SafeFile $_.FullName (Join-Path "$INSTALL_DIR\data" $_.Name)
        }
    }

    $assetsSrc = Join-Path $SourceDir "assets"
    if (Test-Path -LiteralPath $assetsSrc) {
        Get-ChildItem -LiteralPath $assetsSrc -File | ForEach-Object {
            Copy-SafeFile $_.FullName (Join-Path "$INSTALL_DIR\assets" $_.Name)
        }
        $previewsSrc = Join-Path $assetsSrc "previews"
        if (Test-Path -LiteralPath $previewsSrc) {
            Get-ChildItem -LiteralPath $previewsSrc -File | ForEach-Object {
                Copy-SafeFile $_.FullName (Join-Path "$INSTALL_DIR\assets\previews" $_.Name)
            }
        }
    }

    Set-Content -LiteralPath "$INSTALL_DIR\heyu_installed.marker" -Value "installed" -Encoding Ascii
    Set-Content -LiteralPath "$INSTALL_DIR\data\tsf_registered.marker" -Value "registered" -Encoding Ascii

    # Grant Read+Execute to ALL APPLICATION PACKAGES (S-1-15-2-1) and Restricted (S-1-15-2-2) for UWP/Browser sandboxes
    Start-Process -FilePath "$env:SystemRoot\System32\icacls.exe" -ArgumentList "`"$INSTALL_DIR`" /grant `"*S-1-15-2-1:(OI)(CI)(RX)`" /grant `"*S-1-15-2-2:(OI)(CI)(RX)`" /T /Q" -Wait -NoNewWindow -ErrorAction SilentlyContinue | Out-Null

    Write-Host "[3/4] Registering 64-bit and 32-bit TSF Text Service DLLs..." -ForegroundColor Cyan
    $p64 = Start-Process -FilePath "$env:SystemRoot\System32\regsvr32.exe" -ArgumentList "/s `"$INSTALL_DIR\heyu_tsf.dll`"" -Wait -NoNewWindow -PassThru
    if ($p64.ExitCode -ne 0) { throw "64-bit regsvr32 failed with exit code $($p64.ExitCode)" }

    $p32 = Start-Process -FilePath "$env:SystemRoot\SysWOW64\regsvr32.exe" -ArgumentList "/s `"$INSTALL_DIR\heyu_tsf_x86.dll`"" -Wait -NoNewWindow -PassThru
    if ($p32.ExitCode -ne 0) { throw "32-bit regsvr32 failed with exit code $($p32.ExitCode)" }
}

function Invoke-UserStep {
    Write-Host "[4/4] Adding HEYU ($NAME_HEYU_IME) to Windows taskbar language list, Autostart & Desktop shortcuts..." -ForegroundColor Cyan

    # Mark TSF registered in repo data/ and %APPDATA%\HEYU\data
    $repoData = Join-Path $SourceDir "data"
    if (Test-Path -LiteralPath $repoData) {
        Set-Content -LiteralPath (Join-Path $repoData "tsf_registered.marker") -Value "registered" -Encoding Ascii
    }
    $appDataDir = Join-Path $env:APPDATA "HEYU\data"
    New-Item -ItemType Directory -Force -Path $appDataDir | Out-Null
    Set-Content -LiteralPath (Join-Path $appDataDir "tsf_registered.marker") -Value "registered" -Encoding Ascii

    # Seed user data files (keep user_custom.tsv / config.json / hud_style.json if present, update system lexicons)
    if (Test-Path -LiteralPath $repoData) {
        Get-ChildItem -LiteralPath $repoData -File -ErrorAction SilentlyContinue | ForEach-Object {
            $dst = Join-Path $appDataDir $_.Name
            $isUserConfig = ($_.Name -in @("user_custom.tsv", "config.json", "hud_style.json"))
            if ((-not $isUserConfig) -or (!(Test-Path -LiteralPath $dst))) {
                Copy-Item -Force -LiteralPath $_.FullName -Destination $dst
            }
        }
    }

    # APPEND the HEYU TIP to the zh-CN keyboard list. Never remove or reorder existing input methods
    # (Microsoft Pinyin etc. stay installed; the user switches with Win+Space).
    try {
        $list = Get-WinUserLanguageList
        $zh = $list | Where-Object { $_.LanguageTag -like "zh*" } | Select-Object -First 1
        if (-not $zh) {
            $list.Add("zh-CN")
            $zh = $list | Where-Object { $_.LanguageTag -like "zh*" } | Select-Object -First 1
        }
        if ($zh -and ($zh.InputMethodTips -notcontains $TIP_ID)) {
            $zh.InputMethodTips.Add($TIP_ID)
            Set-WinUserLanguageList $list -Force -WarningAction SilentlyContinue
        }
    } catch {
        Write-Warning "Could not update WinUserLanguageList automatically: $_"
    }

    $targetBase = if (Test-Path -LiteralPath "$INSTALL_DIR\heyu-ime.exe") { $INSTALL_DIR } else { $SourceDir }
    $targetExe  = Join-Path $targetBase "heyu-ime.exe"
    $targetPy   = Join-Path $targetBase "heyu_studio.py"
    $targetIco  = Join-Path $targetBase "assets\heyu.ico"

    # Configure HKCU Run Autostart
    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    Set-ItemProperty -Path $runKey -Name "HeyuIME" -Value "`"$targetExe`""

    # Create Desktop & Startup shortcuts via WScript.Shell
    $wsh = New-Object -ComObject WScript.Shell
    $desktop = [Environment]::GetFolderPath("Desktop")
    $startup = [Environment]::GetFolderPath("Startup")

    # 1. Startup shortcut
    $scStartup = $wsh.CreateShortcut((Join-Path $startup "$NAME_HEYU_IME.lnk"))
    $scStartup.TargetPath = $targetExe
    $scStartup.WorkingDirectory = $targetBase
    if (Test-Path -LiteralPath $targetIco) { $scStartup.IconLocation = "$targetIco,0" }
    $scStartup.Description = $NAME_HEYU_IME
    $scStartup.Save()

    # 2. Desktop shortcut: Engine
    $scStart = $wsh.CreateShortcut((Join-Path $desktop $NAME_START_LNK))
    $scStart.TargetPath = $targetExe
    $scStart.WorkingDirectory = $targetBase
    if (Test-Path -LiteralPath $targetIco) { $scStart.IconLocation = "$targetIco,0" }
    $scStart.Description = $NAME_HEYU_IME
    $scStart.Save()

    # 3. Desktop shortcut: Studio Control Panel
    $pyw = (Get-Command pythonw.exe -ErrorAction SilentlyContinue).Source
    if (-not $pyw) { $pyw = "pythonw.exe" }
    $scStudio = $wsh.CreateShortcut((Join-Path $desktop $NAME_STUDIO_LNK))
    $scStudio.TargetPath = $pyw
    $scStudio.Arguments = "`"$targetPy`""
    $scStudio.WorkingDirectory = $targetBase
    if (Test-Path -LiteralPath $targetIco) { $scStudio.IconLocation = "$targetIco,0" }
    $scStudio.Description = $NAME_HEYU_IME
    $scStudio.Save()

    # Stop any stale instance and launch the server in the user's interactive desktop session
    Get-Process -Name "heyu-ime" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 200
    try {
        $act = New-ScheduledTaskAction -Execute $targetExe -WorkingDirectory $targetBase
        $trig = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
        $prin = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive
        $set  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
        Register-ScheduledTask -TaskName "HeyuImeLauncher" -Action $act -Trigger $trig -Principal $prin -Settings $set -Force | Out-Null
        Start-ScheduledTask -TaskName "HeyuImeLauncher"
    } catch {
        Start-Process -FilePath $targetExe -WorkingDirectory $targetBase
    }

    Write-Host ""
    Write-Host "================================================================" -ForegroundColor Green
    Write-Host "  HEYU ($NAME_HEYU_IME) Installed & Registered Successfully!" -ForegroundColor Green
    Write-Host "  - Install Location     : $targetBase" -ForegroundColor Green
    Write-Host "  - Taskbar IME Selector : Added under Chinese (Simplified) [Win + Space]" -ForegroundColor Green
    Write-Host "  - Windows Autostart    : Enabled (HKCU Run + Startup folder)" -ForegroundColor Green
    Write-Host "  - Desktop Shortcuts    : $NAME_STUDIO_LNK & $NAME_START_LNK" -ForegroundColor Green
    Write-Host "================================================================" -ForegroundColor Green
}

if ($UserStepOnly) {
    Invoke-UserStep
    exit 0
}

if ($ElevatedStep) {
    try {
        Invoke-ElevatedStep
        exit 0
    } catch {
        $errText = "ERROR in Invoke-ElevatedStep: $($_.Exception.Message)`r`n$($_.InvocationInfo.PositionMessage)`r`n$($_.ScriptStackTrace)"
        Write-Host $errText -ForegroundColor Red
        if ($LogFile -and $LogFile -ne "") {
            Set-Content -LiteralPath $LogFile -Value $errText -Encoding UTF8 -ErrorAction SilentlyContinue
        }
        exit 1
    }
}

Write-Host "================================================================" -ForegroundColor Cyan
Write-Host "  HEYU ($NAME_HEYU_IME) One-Click Installer & TSF Registration" -ForegroundColor Cyan
Write-Host "================================================================" -ForegroundColor Cyan

# Pre-Installation Mandatory Master Password & Capsule Identity Setup Wizard
$cfgRepo = Join-Path $SourceDir "data\config.json"
$cfgApp  = Join-Path $env:APPDATA "HEYU\data\config.json"
$hasMasterPwd = $false
foreach ($cp in @($cfgApp, $cfgRepo)) {
    if (Test-Path -LiteralPath $cp) {
        try {
            $rawCfg = Get-Content -LiteralPath $cp -Raw -Encoding UTF8 | ConvertFrom-Json
            if ($rawCfg.master_pwd_verifier -and $rawCfg.master_pwd_verifier.ToString().Trim().Length -gt 0) {
                $hasMasterPwd = $true
                break
            }
        } catch {}
    }
}
if (-not $hasMasterPwd) {
    Write-Host "[0/4] Launching Pre-Install Security Master Password & Capsule Identity Wizard..." -ForegroundColor Yellow
    $pyExe = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
    $studioPy = Join-Path $SourceDir "heyu_studio.py"
    if ($pyExe -and (Test-Path -LiteralPath $studioPy)) {
        Start-Process -FilePath $pyExe -ArgumentList "`"$studioPy`" --setup-wizard" -WorkingDirectory $SourceDir -Wait
    }
}

if (Test-IsAdmin) {
    Invoke-ElevatedStep
    Invoke-UserStep
} else {
    Write-Host "Requesting Administrator permission (UAC) to register TSF DLLs in C:\Program Files\HEYU ..." -ForegroundColor Yellow
    $logFile = Join-Path $env:TEMP "heyu_install_elevated.log"
    Remove-Item -LiteralPath $logFile -Force -ErrorAction SilentlyContinue

    $scriptPath = $PSCommandPath
    if (-not $scriptPath) { $scriptPath = Join-Path $SourceDir "install.ps1" }
    $cmd = "& '$($scriptPath.Replace("'", "''"))' -ElevatedStep -SourceDir '$($SourceDir.Replace("'", "''"))' -LogFile '$($logFile.Replace("'", "''"))'"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cmd))

    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-EncodedCommand", $encoded) -Verb RunAs -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        if (Test-Path -LiteralPath $logFile) {
            Write-Host "Details from elevated installer:" -ForegroundColor Red
            Write-Host (Get-Content -LiteralPath $logFile -Raw -Encoding UTF8) -ForegroundColor Red
        }
        throw "Elevated installation step failed (exit code $($proc.ExitCode))."
    }
    Invoke-UserStep
}
