# Build GPUIdium from the working tree and install it per user under %LOCALAPPDATA%\Programs\GPUIdium,
# with a Start menu shortcut and an entry in Settings > Apps (uninstall there or run uninstall.ps1).
# Uses the normal target\ dir, so builds share the cache with `task build:release` / `task run`.
# Runtime dep (not installed): Microsoft Edge WebView2 Runtime (part of Windows 11).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)
$id = 'io.github.rpietzsch.GPUIdium'
$ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$dir = Join-Path $env:LOCALAPPDATA 'Programs\GPUIdium'
$exe = Join-Path $dir 'gpuidium.exe'

cargo build --release -p gpuidium --bin gpuidium
if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
if (Get-Process gpuidium -ErrorAction SilentlyContinue | Where-Object Path -eq $exe) { throw "GPUIdium is running from $dir; quit it first" }

New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item target\release\gpuidium.exe $exe -Force
Copy-Item scripts\uninstall-windows.ps1 (Join-Path $dir 'uninstall.ps1') -Force

# .ico with PNG-compressed entries (supported since Vista) from the existing app icons.
$pngs = 32, 64, 128 | ForEach-Object { , [IO.File]::ReadAllBytes("crates\gpuidium\assets\icon-$_.png") }
$ms = New-Object IO.MemoryStream
$w = New-Object IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$pngs.Count)
$offset = 6 + 16 * $pngs.Count
$sizes = 32, 64, 128
for ($i = 0; $i -lt $pngs.Count; $i++) {
    $w.Write([byte]$sizes[$i]); $w.Write([byte]$sizes[$i]); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write($p) }
$ico = Join-Path $dir 'gpuidium.ico'
[IO.File]::WriteAllBytes($ico, $ms.ToArray())

$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'GPUIdium.lnk'
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = $exe; $s.WorkingDirectory = $dir; $s.IconLocation = "$ico,0"; $s.Description = 'GPUIdium'
$s.Save()

$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$id"
New-Item -Force $key | Out-Null
$kb = [int]((Get-ChildItem $dir | Measure-Object Length -Sum).Sum / 1KB)
@{
    DisplayName     = 'GPUIdium'
    DisplayVersion  = $ver
    Publisher       = 'rpietzsch'
    URLInfoAbout    = 'https://github.com/rpietzsch/gpuidium'
    DisplayIcon     = $ico
    InstallLocation = $dir
    UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dir\uninstall.ps1`""
}.GetEnumerator() | ForEach-Object { Set-ItemProperty $key $_.Key $_.Value }
foreach ($n in 'NoModify', 'NoRepair') { New-ItemProperty $key $n -Value 1 -PropertyType DWord -Force | Out-Null }
New-ItemProperty $key EstimatedSize -Value $kb -PropertyType DWord -Force | Out-Null

Write-Host "installed GPUIdium $ver to $dir (Start menu: GPUIdium; remove via Settings > Apps)"
