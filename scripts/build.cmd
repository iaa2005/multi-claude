@echo off
rem Builds a local MultiClaude installer with the MSVC toolchain loaded.
rem
rem Updater artifacts are turned off: they must be signed with the upstream
rem CC Switch key, which this fork does not have. Output lands in
rem src-tauri\target\release\bundle\nsis\ (installer) and
rem src-tauri\target\release\ (portable exe).
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
  echo [build] vcvars64.bat failed to load 1>&2
  exit /b 1
)
cd /d "%~dp0.."
pnpm tauri build --config "{\"bundle\":{\"createUpdaterArtifacts\":false}}" %*
