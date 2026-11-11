@echo off
rem Builds the Windows installer: desktop\dist\Agent Orchestrator Setup <version>.exe
rem Needs Python 3.12 and Node on the BUILD machine only; the installed app bundles its own backend and UI.
rem Everything is built in %TEMP%\ao-build so a linked node_modules in the checkout cannot leak in or be skipped.
setlocal
set ROOT=%~dp0
set B=%TEMP%\ao-build
rmdir /s /q "%B%" 2>nul
mkdir "%B%\frontend" || exit /b 1

echo [1/4] Backend: freezing with PyInstaller
python -m pip install -r "%ROOT%backend\requirements.txt" pyinstaller || exit /b 1
pushd "%ROOT%backend"
python -m PyInstaller --noconfirm --onedir --name ao-backend --distpath "%B%\dist" --workpath "%B%\work" --specpath "%B%" --paths . --collect-submodules app --collect-submodules uvicorn --hidden-import uvicorn.logging --hidden-import uvicorn.loops.auto --hidden-import uvicorn.protocols.http.auto --hidden-import uvicorn.protocols.websockets.auto --hidden-import uvicorn.lifespan.on server.py || exit /b 1
popd
rem built-in roles and plugins are read relative to the backend folder
xcopy /e /i /q "%ROOT%.development" "%B%\dist\ao-backend\.development" >nul || exit /b 1
xcopy /e /i /q "%ROOT%plugins" "%B%\dist\ao-backend\plugins" >nul || exit /b 1

echo [2/4] UI: standalone Next build (backend port 8765 is baked in)
robocopy "%ROOT%frontend" "%B%\frontend" /e /xd node_modules .next >nul
pushd "%B%\frontend"
call npm ci || exit /b 1
set AO_BACKEND_URL=http://127.0.0.1:8765
call npm run build || exit /b 1
popd

echo [3/4] Staging
rmdir /s /q "%ROOT%desktop\stage" 2>nul
xcopy /e /i /q "%B%\dist\ao-backend" "%ROOT%desktop\stage\backend" >nul || exit /b 1
xcopy /e /i /q "%B%\frontend\.next\standalone" "%ROOT%desktop\stage\ui" >nul || exit /b 1
xcopy /e /i /q "%B%\frontend\.next\static" "%ROOT%desktop\stage\ui\.next\static" >nul || exit /b 1

echo [4/4] Installer
pushd "%ROOT%desktop"
call npm install || exit /b 1
call npm run dist || exit /b 1
popd
echo Done: desktop\dist\Agent Orchestrator Setup *.exe
