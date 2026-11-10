@echo off
chcp 65001 >nul
taskkill /F /IM heyu-ime.exe >nul 2>&1
echo 鹤羽输入法后台引擎 (heyu-ime.exe) 已安全退出。
timeout /t 1 >nul
