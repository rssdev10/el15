@echo off
rem Install the EL15 SCPI server as a Windows service using sc.exe.
rem Run from an elevated (Administrator) command prompt.
rem
rem Usage:   install-service.bat [path\to\el15.exe] [port]
rem Default: el15.exe in PATH, port 5555
rem
rem Note: sc.exe-created services run as LocalSystem by default. If your
rem Bluetooth adapter is per-user, prefer Task Scheduler with "At log on"
rem trigger and "Run only when user is logged on" instead (see README).

setlocal
set EL15_BIN=%~1
if "%EL15_BIN%"=="" set EL15_BIN=el15.exe
set PORT=%~2
if "%PORT%"=="" set PORT=5555

echo Installing el15 service...
echo   binary: %EL15_BIN%
echo   port  : %PORT%

sc create EL15 binPath= "\"%EL15_BIN%\" --no-gui --port %PORT%" start= auto DisplayName= "ALIENTEK EL15 SCPI server"
if errorlevel 1 (
    echo.
    echo Failed to create service. Make sure you are running as Administrator.
    exit /b 1
)

sc description EL15 "Background TCP SCPI server bridging to the EL15 DC electronic load over BLE."
sc start EL15

echo.
echo Service 'EL15' installed and started.
echo   stop      : sc stop EL15
echo   uninstall : uninstall-service.bat
endlocal
