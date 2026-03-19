@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0ci.ps1" %*
exit /b %errorlevel%
