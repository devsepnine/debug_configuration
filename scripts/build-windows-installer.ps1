param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Command,
        [Parameter(Mandatory = $true)]
        [string]$ErrorMessage
    )

    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw $ErrorMessage
    }
}

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$BinaryName = "run_config_manager"
$PackageName = "run_config_manager-installer.msi"
$WixVersion = "6.0.2"
$TargetDir = Join-Path $ProjectRoot "target\packaging"
$ExePath = Join-Path $ProjectRoot "target\$Configuration\$BinaryName.exe"
$WxsPath = Join-Path $ProjectRoot "wix\main.wxs"
$OutputPath = Join-Path $TargetDir $PackageName

if (-not (Test-Path $ExePath)) {
    Invoke-NativeCommand {
        cargo build --release --locked
    } "Release build failed"
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
    Invoke-NativeCommand {
        dotnet tool install --global wix --version $WixVersion
    } "WiX tool installation failed"
    $DotnetTools = Join-Path $env:USERPROFILE ".dotnet\tools"
    $env:PATH = "$env:PATH;$DotnetTools"
}

Invoke-NativeCommand {
    wix extension add "WixToolset.UI.wixext/$WixVersion" --global
} "WiX UI extension installation failed"

Invoke-NativeCommand {
    wix extension add "WixToolset.Util.wixext/$WixVersion" --global
} "WiX Util extension installation failed"

Invoke-NativeCommand {
    wix build `
        -pdbtype none `
        -arch x64 `
        -ext WixToolset.UI.wixext `
        -ext WixToolset.Util.wixext `
        -d "PackageVersion=$PackageVersion" `
        -d "SourceDir=$ProjectRoot" `
        $WxsPath `
        -o $OutputPath
} "Windows installer build failed"

Write-Host "Windows installer created at: $OutputPath"
