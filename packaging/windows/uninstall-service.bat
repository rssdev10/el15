@echo off
rem Uninstall the EL15 Windows service. Run as Administrator.
sc stop EL15 >nul 2>&1
sc delete EL15
echo Service EL15 removed.
