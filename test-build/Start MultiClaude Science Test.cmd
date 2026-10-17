@echo off
rem Runs the Claude Science test build beside the app that is in use.
rem
rem Its own config dir: database, live-state, settings and logs are separate.
rem Its own Science data dir: switching providers restarts the daemon and must
rem not touch the real research data in %USERPROFILE%\.claude-science.
set "MULTICLAUDE_CONFIG_DIR=%USERPROFILE%\.multiclaude-science-test"
set "CLAUDE_SCIENCE_DATA_DIR=%USERPROFILE%\.claude-science-test"
start "" "%~dp0MultiClaudeScienceTest.exe"
