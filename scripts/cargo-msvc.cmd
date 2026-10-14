@echo off
rem Runs cargo with the MSVC toolchain on PATH.
rem
rem Git Bash puts GNU coreutils ahead of MSVC in PATH, so plain `cargo check`
rem links with /usr/bin/link (the hardlink tool) and fails with
rem "link: extra operand". This wrapper loads vcvars64 first so link.exe
rem resolves to the real MSVC linker.
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
  echo [cargo-msvc] vcvars64.bat failed to load 1>&2
  exit /b 1
)
cd /d "%~dp0..\src-tauri"
cargo %*
