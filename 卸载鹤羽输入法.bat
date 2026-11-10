@echo off
cd /d "%~dp0"
where pythonw >nul 2>nul
if %errorlevel%==0 (
    start "" pythonw ".\heyu_installer.py" --uninstall
    exit /b 0
)
where python >nul 2>nul
if %errorlevel%==0 (
    python ".\heyu_installer.py" --uninstall
    exit /b 0
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\uninstall.ps1"
echo.
pause
