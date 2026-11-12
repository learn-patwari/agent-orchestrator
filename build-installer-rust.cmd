@echo off
rem Builds the Rust desktop installer: rust\target\release\bundle\nsis\Agent Orchestrator_<version>_x64-setup.exe
rem Needs Rust, Node and the Tauri CLI (cargo install tauri-cli --version "^2") on the BUILD machine only.
rem The installed app bundles the backend, the UI, the built-in roles and the shipped plugins; nothing else to install.
setlocal
set ROOT=%~dp0
call "%ROOT%build-frontend-static.cmd" || exit /b 1
pushd "%ROOT%rust\ao-desktop"
cargo tauri build --bundles nsis || exit /b 1
popd
echo Done: rust\target\release\bundle\nsis\
