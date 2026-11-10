@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo ========================================================
echo   🪶 正在启动 鹤羽输入法 (HEYU Rust 原生净室引擎)...
echo ========================================================
start "" "%~dp0heyu-ime.exe"
echo   ✅ 鹤羽输入法已启动！(桌面右下角悬浮胶囊与系统托盘已就绪)
echo   💡 单按 Left Shift 切换中/英，Ctrl+Shift+L 切换中/意/西/英
timeout /t 2 >nul
