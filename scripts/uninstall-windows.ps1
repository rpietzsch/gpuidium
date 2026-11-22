# Remove a per-user GPUIdium install made by scripts/install-windows.ps1 (copied there as uninstall.ps1).
# Settings and app data (profiles, logins) are kept.
$ErrorActionPreference = 'Stop'
$id = 'io.github.rpietzsch.GPUIdium'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\GPUIdium'
if (Get-Process gpuidium -ErrorAction SilentlyContinue | Where-Object Path -eq (Join-Path $dir 'gpuidium.exe')) { throw 'GPUIdium is running; quit it first' }
Set-Location $env:TEMP
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path ([Environment]::GetFolderPath('Programs')) 'GPUIdium.lnk')
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$id"
# Toast sender registered by the app at startup.
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue "HKCU:\Software\Classes\AppUserModelId\$id"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $dir
Write-Host "removed $dir"
