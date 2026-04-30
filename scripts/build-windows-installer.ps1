param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$BinaryName = "run_config_manager"
$PackageName = "run_config_manager-installer.msi"
$TargetDir = Join-Path $ProjectRoot "target\packaging"
$ExePath = Join-Path $ProjectRoot "target\$Configuration\$BinaryName.exe"
$WxsPath = Join-Path $ProjectRoot "wix\main.wxs"
$OutputPath = Join-Path $TargetDir $PackageName

if (-not (Test-Path $ExePath)) {
    cargo build --release --locked
}

$Manifest = Get-Content (Join-Path $ProjectRoot "Cargo.toml")
$VersionLine = $Manifest | Select-String -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $VersionLine) {
    throw "Could not read package version from Cargo.toml"
}

$PackageVersion = $VersionLine.Matches[0].Groups[1].Value
if (($PackageVersion.Split(".")).Count -lt 3) {
    $PackageVersion = "$PackageVersion.0"
}

New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    dotnet tool install --global wix --version 6.0.2
    $DotnetTools = Join-Path $env:USERPROFILE ".dotnet\tools"
    $env:PATH = "$env:PATH;$DotnetTools"
}

wix build `
    -pdbtype none `
    -arch x64 `
    -d "PackageVersion=$PackageVersion" `
    -d "SourceDir=$ProjectRoot" `
    $WxsPath `
    -o $OutputPath

Write-Host "Windows installer created at: $OutputPath"
