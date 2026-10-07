@echo off
rem Signs one Windows file with ssign and keeps what ssign says.
rem
rem Tauri runs this for every executable it bundles (tauri.signing.conf.json),
rem and it shows nothing of a sign command that failed: v26.10.11-rc2 ended with
rem "failed to run ssign" and no reason. So everything ssign prints goes to a
rem log that the release workflow prints afterwards, and the exit code is
rem ssign's own.
rem
rem It is a .cmd because the uninstaller is signed from inside makensis, which
rem starts the command through the shell and from another directory.
echo ---- %~nx1>> "%RUNNER_TEMP%\ssign.log"
ssign --verbose %1 >> "%RUNNER_TEMP%\ssign.log" 2>&1
exit /b %ERRORLEVEL%
