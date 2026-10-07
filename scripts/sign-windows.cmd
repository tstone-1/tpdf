@echo off
rem Tauri's sign command (tauri.signing.conf.json). It is a .cmd because the
rem uninstaller is signed from inside makensis, which starts the command through
rem the shell and from another directory. The work is in sign-windows.ps1,
rem which the release workflow puts beside this file.
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0sign-windows.ps1" %1
exit /b %ERRORLEVEL%
