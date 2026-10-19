@echo off
rem Builds a local MultiClaude installer with the MSVC toolchain loaded, then
rem signs it for the updater.
rem
rem The updater key is MultiClaude's own, kept outside the repository at
rem %USERPROFILE%\.tauri\multiclaude-updater.key (no password; the public half
rem is in tauri.conf.json). Tauri's in-build signing prompts for the password
rem when the variable is empty, which cmd cannot set, so the installer is
rem signed afterwards with `tauri signer sign`. In Tauri 2 the NSIS installer
rem is the update payload: its .sig is all latest.json needs.
rem
rem Output: src-tauri\target\release\bundle\nsis\ (installer + .sig) and
rem src-tauri\target\release\ (portable exe).
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
  echo [build] vcvars64.bat failed to load 1>&2
  exit /b 1
)
cd /d "%~dp0.."
call pnpm tauri build --config "{\"bundle\":{\"createUpdaterArtifacts\":false}}" %*
if errorlevel 1 exit /b 1

set "KEY_FILE=%USERPROFILE%\.tauri\multiclaude-updater.key"
if not exist "%KEY_FILE%" (
  echo [build] %KEY_FILE% not found: the installer is not signed for the updater 1>&2
  exit /b 0
)
for /f "delims=" %%v in ('node -p "require('./package.json').version"') do set "VERSION=%%v"
set "INSTALLER=src-tauri\target\release\bundle\nsis\MultiClaude_%VERSION%_x64-setup.exe"
call pnpm tauri signer sign -f "%KEY_FILE%" -p "" "%INSTALLER%" < nul
if errorlevel 1 (
  echo [build] signing %INSTALLER% failed 1>&2
  exit /b 1
)
echo [build] signed %INSTALLER%
