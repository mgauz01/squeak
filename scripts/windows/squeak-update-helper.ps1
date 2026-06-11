# Elevated MSI install + relaunch (spawned detached from Squeak before exit).
param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $MsiPath)) {
    throw "MSI not found: $MsiPath"
}

$proc = Start-Process -FilePath 'msiexec.exe' `
    -ArgumentList @('/i', $MsiPath, '/passive', '/norestart') `
    -Verb RunAs -Wait -PassThru

if ($proc.ExitCode -ne 0) {
    exit $proc.ExitCode
}

$squeak = Join-Path $env:ProgramFiles 'Squeak\squeak.exe'
if (-not (Test-Path -LiteralPath $squeak)) {
    throw "Squeak executable not found at $squeak"
}

Start-Process -FilePath $squeak
