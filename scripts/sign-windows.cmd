@echo off
rem Tauri's sign command (tauri.signing.conf.json). It is a .cmd because the
rem uninstaller is signed from inside makensis, which starts the command through
rem the shell and from another directory. The work is in sign-windows.ps1.
rem
rem The script is named by TPDF_SIGN_SCRIPT and not found beside this file:
rem makensis starts this file by its quoted name through PATH, and cmd then
rem gives %~dp0 as the current folder. 26.10.11 shipped an unsigned uninstaller
rem for that: "UninstFinalize command returned 64", which makensis does not
rem treat as an error.
if not defined TPDF_SIGN_SCRIPT (
  echo TPDF_SIGN_SCRIPT is not set 1>&2
  exit /b 1
)
pwsh -NoProfile -ExecutionPolicy Bypass -File "%TPDF_SIGN_SCRIPT%" %1
exit /b %ERRORLEVEL%
