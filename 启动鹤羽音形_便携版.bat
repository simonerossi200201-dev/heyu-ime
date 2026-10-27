@echo off
chcp 65001 >nul
cd /d "%~dp0"
if not exist "data" mkdir "data"
if not exist "data\voice_tier.txt" if exist "tools\select_voice_tier.ps1" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\select_voice_tier.ps1" -DataDir "%~dp0data"
start "" "%~dp0heyu-ime.exe" --hook
if exist "%~dp0heyu-voice.exe" (
    start "" "%~dp0heyu-voice.exe"
)
where pythonw >nul 2>nul
if %errorlevel% equ 0 (
    start "" pythonw "%~dp0heyu_studio.py"
) else (
    where python >nul 2>nul
    if %errorlevel% equ 0 (
        start "" python "%~dp0heyu_studio.py"
    )
)
exit /b 0
