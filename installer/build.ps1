#Requires -Version 5.1
<#
.SYNOPSIS
  Build a release binary and package it as an x64 MSI.

.DESCRIPTION
  1. cargo build --release
  2. Stage squeak.exe + runtime DLLs
  3. heat harvest DLL components
  4. candle + light (WiX 3.14)

  Prerequisites (Windows):
    - Rust stable (MSVC toolchain)
    - WiX Toolset 3.14 (pick one):
        choco install wixtoolset --version=3.14.1 -y
        winget install WiXToolset.WiXToolset

.EXAMPLE
  .\installer\build.ps1
  .\installer\build.ps1 -Features "parakeet,gec-tiny"
#>
[CmdletBinding()]
param(
    [string]$Features = "parakeet",
    [ValidateSet("release", "debug")]
    [string]$Configuration = "release"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Find-WixTool {
    param([Parameter(Mandatory)][string]$Name)

    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    $searchRoots = @(
        "${env:ProgramFiles(x86)}\WiX Toolset v3.14\bin",
        "${env:ProgramFiles}\WiX Toolset v3.14\bin",
        "${env:ProgramFiles(x86)}\WiX Toolset v3.11\bin"
    )

    foreach ($root in $searchRoots) {
        $candidate = Join-Path $root "$Name.exe"
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    throw @"
WiX '$Name' not found on PATH.

Install WiX Toolset 3.14 (admin PowerShell), then reopen your terminal:

  choco install wixtoolset --version=3.14.1 -y
  # or: winget install WiXToolset.WiXToolset
  # or: https://github.com/wixtoolset/wix3/releases/tag/wix3141rtm
"@
}

function Get-CargoPackageInfo {
    $metadataJson = cargo metadata --format-version 1 --no-deps
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed"
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $pkg = $metadata.packages | Where-Object { $_.name -eq "squeak" } | Select-Object -First 1
    if (-not $pkg) {
        throw "Could not find squeak package in cargo metadata"
    }
    return $pkg
}

function ConvertTo-WixVersion {
    param([string]$Version)

    if ($Version -match '^\d+\.\d+\.\d+\.\d+$') {
        return $Version
    }
    if ($Version -match '^(\d+\.\d+\.\d+)$') {
        return "$Version.0"
    }
    throw "Unsupported Cargo version '$Version' for WiX ProductVersion (need major.minor.patch[.build])"
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$candle = Find-WixTool -Name "candle"
$light = Find-WixTool -Name "light"
$heat = Find-WixTool -Name "heat"

$pkg = Get-CargoPackageInfo
$productVersion = ConvertTo-WixVersion -Version $pkg.version
$manufacturer = if (@($pkg.authors).Count -gt 0) { $pkg.authors[0] } else { "Squeak Contributors" }

Write-Host "Building squeak $productVersion (features: $Features)..." -ForegroundColor Cyan

$featureArgs = @()
if ($Features.Trim()) {
    $featureArgs = @("--features", $Features)
}

cargo build --$Configuration @featureArgs
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed"
}

$targetDir = Join-Path $repoRoot "target"
$releaseDir = Join-Path $targetDir "$Configuration"
if (-not (Test-Path (Join-Path $releaseDir "squeak.exe"))) {
  $releaseDir = Join-Path $targetDir "x86_64-pc-windows-msvc\$Configuration"
}
if (-not (Test-Path (Join-Path $releaseDir "squeak.exe"))) {
    throw "squeak.exe not found under target\ (expected MSVC Windows build output)"
}

$distDir = Join-Path $repoRoot "dist"
$stagingDir = Join-Path $distDir "staging"
$wixDir = Join-Path $repoRoot "installer\wix"
$harvestedWxs = Join-Path $wixDir "Harvested.wxs"
$msiName = "Squeak-$($pkg.version)-x64.msi"
$msiPath = Join-Path $distDir $msiName

if (Test-Path $stagingDir) {
    Remove-Item -Recurse -Force $stagingDir
}
New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

Copy-Item (Join-Path $releaseDir "squeak.exe") $stagingDir

$dlls = @(Get-ChildItem -Path $releaseDir -Filter "*.dll" -File)
if ($dlls.Count -eq 0) {
    Write-Warning "No DLLs found next to squeak.exe; MSI will contain only the executable."
} else {
    $dllStaging = Join-Path $stagingDir "dlls"
    New-Item -ItemType Directory -Force -Path $dllStaging | Out-Null
    $dlls | Copy-Item -Destination $dllStaging
}

if (Test-Path $harvestedWxs) {
    Remove-Item -Force $harvestedWxs
}

if ($dlls.Count -gt 0) {
    $dllStaging = Join-Path $stagingDir "dlls"
    & $heat dir $dllStaging `
        -cg HarvestedDlls `
        -dr INSTALLFOLDER `
        -var var.DllDir `
        -sfrag `
        -srd `
        -gg `
        -ag `
        -out $harvestedWxs
    if ($LASTEXITCODE -ne 0) {
        throw "heat failed"
    }
} else {
    @"
<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <ComponentGroup Id="HarvestedDlls" />
  </Fragment>
</Wix>
"@ | Set-Content $harvestedWxs -Encoding UTF8
}

$wixobjDir = Join-Path $distDir "wixobj"
if (Test-Path $wixobjDir) {
    Remove-Item -Recurse -Force $wixobjDir
}
New-Item -ItemType Directory -Force -Path $wixobjDir | Out-Null

$mainWxs = Join-Path $wixDir "squeak.wxs"
$candleDefines = @(
    "-dProductVersion=$productVersion",
    "-dProductManufacturer=$manufacturer",
    "-dStagingDir=$stagingDir"
)
if ($dlls.Count -gt 0) {
    $dllStaging = Join-Path $stagingDir "dlls"
    $candleDefines += "-dDllDir=$dllStaging"
}

$candleArgs = @(
    "-nologo",
    "-arch", "x64"
) + $candleDefines + @(
    "-out", (Join-Path $wixobjDir "\"),
    $mainWxs,
    $harvestedWxs
)

& $candle @candleArgs
if ($LASTEXITCODE -ne 0) {
    throw "candle failed"
}

$wixobjs = @(Get-ChildItem $wixobjDir -Filter "*.wixobj")
if ($wixobjs.Count -eq 0) {
    throw "No .wixobj files produced"
}

if (Test-Path $msiPath) {
    Remove-Item -Force $msiPath
}

$lightArgs = @(
    "-nologo",
    "-ext", "WixUtilExtension",
    "-out", $msiPath
) + ($wixobjs | ForEach-Object { $_.FullName })

& $light @lightArgs
if ($LASTEXITCODE -ne 0) {
    throw "light failed"
}

Write-Host ""
Write-Host "MSI ready: $msiPath" -ForegroundColor Green
Write-Host "Install (elevated): msiexec /i `"$msiPath`""
