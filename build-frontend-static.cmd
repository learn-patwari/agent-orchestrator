@echo off
rem Builds the UI as static files (frontend\out) for the Rust desktop app, which serves them itself.
set AO_STATIC=1
cd /d "%~dp0frontend" || exit /b 1
call npm ci || exit /b 1
call npm run build || exit /b 1
echo Static UI built in frontend\out.
