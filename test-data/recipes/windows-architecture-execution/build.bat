@echo off

echo PROCESSOR_ARCHITECTURE=%PROCESSOR_ARCHITECTURE%
if /I not "%PROCESSOR_ARCHITECTURE%" == "ARM64" exit /b 64
if not "%PROCESSOR_ARCHITEW6432%" == "" exit /b 65

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0verify-arm64.ps1"
if errorlevel 1 exit /b %errorlevel%

rem This deliberate failure verifies that the outer cmd.exe forwards child status.
exit /b 37
