# Install Inno Setup, which build-installer.sh uses to make the Windows installer, for
# the current user (no administrator rights needed).
#
# The release is pinned and its checksum checked, so every build uses exactly the
# version that was tested; a changed download stops the build. To move to a newer
# release, change $Version, $Url and $Sha256 together (the SHA-256 GitHub shows for the
# file on https://github.com/jrsoftware/issrc/releases).
#
# Usage: powershell -File packaging\windows\install-inno-setup.ps1

$ErrorActionPreference = 'Stop'

$Version = '7.1.0'
$Url = 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_0/innosetup-7.1.0-x64.exe'
$Sha256 = '0362a383ed217d4c4239b5933866dd96d3eb2102737da92f80f6057a4b40df2f'

$installer = Join-Path ([IO.Path]::GetTempPath()) "innosetup-$Version-x64.exe"
Write-Host "==> Downloading Inno Setup $Version"
Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $installer
$actual = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLower()
if ($actual -ne $Sha256) { throw "Inno Setup download has SHA-256 $actual, expected $Sha256" }

Write-Host "==> Installing Inno Setup $Version"
$process = Start-Process -FilePath $installer -Wait -PassThru `
    -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER'
if ($process.ExitCode -ne 0) { throw "Inno Setup's installer failed (exit code $($process.ExitCode))" }
Remove-Item $installer

$iscc = Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7\ISCC.exe'
if (-not (Test-Path $iscc)) { throw "ISCC.exe not found at $iscc" }
Write-Host "==> Installed $iscc"
