@echo off
chcp 65001 >nul
cd /d "%~dp0"
start "" "%~dp0heyu-ime.exe"
start "" pythonw.exe "%~dp0heyu_studio.py"
