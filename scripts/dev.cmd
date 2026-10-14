@echo off
rem Runs `pnpm tauri dev` with the MSVC toolchain loaded.
rem
rem Without vcvars64, cc-rs cannot find cl.exe and Git's /usr/bin/link.exe
rem shadows the MSVC linker, so the Rust build fails with exit code 101.
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
  echo [dev] vcvars64.bat failed to load 1>&2
  exit /b 1
)
cd /d "%~dp0.."
pnpm tauri dev %*
