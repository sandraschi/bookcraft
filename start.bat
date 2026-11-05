@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
if errorlevel 1 (
  echo [bookcraft] exited with error - window held open
  pause
  exit /b 1
)
endlocal
exit /b 0
