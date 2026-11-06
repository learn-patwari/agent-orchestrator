@echo off
rem Builds the UI for the desktop app. The backend port is baked in at build time (8765 by default).
set AO_BACKEND_URL=http://127.0.0.1:8765
cd /d "%~dp0frontend" || exit /b 1
call npm ci || exit /b 1
call npm run build || exit /b 1
echo UI built.
