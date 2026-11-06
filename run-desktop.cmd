@echo off
rem Starts the Agent Orchestrator desktop app. First run: installs Python and Node packages and builds the UI.
cd /d "%~dp0"
if not exist backend\.deps-ok (
  python -m pip install -r backend\requirements.txt || exit /b 1
  echo ok> backend\.deps-ok
)
if not exist frontend\.next call build-frontend.cmd || exit /b 1
if not exist desktop\node_modules (
  pushd desktop
  call npm install || exit /b 1
  popd
)
cd desktop && call npm start
