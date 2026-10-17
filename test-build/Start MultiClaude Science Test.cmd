@echo off
rem Runs the Claude Science test build beside the app that is in use.
rem
rem CC_SWITCH_TEST_HOME moves the device state too. DeviceStore deliberately
rem ignores the config-dir override (a synced config dir must not hold the
rem "who owns this client" facts), so without this the test build would read
rem and rewrite the real ~/.multiclaude/live-state.json and then the real
rem Claude Desktop profile.
rem
rem The copied live-state marks every client direct: startup then has nothing
rem to rewrite for Claude/Codex/Gemini/Grok, and the proxy is never started
rem on the old port. Only an explicit switch in this window touches Science.
set "TEST_HOME=%USERPROFILE%\.multiclaude-science-test-home"
set "CC_SWITCH_TEST_HOME=%TEST_HOME%"
set "MULTICLAUDE_CONFIG_DIR=%TEST_HOME%\.multiclaude"
set "CLAUDE_SCIENCE_DATA_DIR=%USERPROFILE%\.claude-science-test"
if not exist "%TEST_HOME%\.multiclaude" mkdir "%TEST_HOME%\.multiclaude"
if not exist "%TEST_HOME%\.multiclaude\live-state.json" copy /Y "%~dp0live-state.direct.json" "%TEST_HOME%\.multiclaude\live-state.json" >nul
start "" "%~dp0MultiClaudeScienceTest.exe"
