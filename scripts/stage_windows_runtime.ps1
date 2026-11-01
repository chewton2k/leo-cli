$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -property installationPath
if (-not $vs) { throw 'Visual Studio was not found for runtime bundling.' }
$version = (Get-Content (Join-Path $vs 'VC\Auxiliary\Build\Microsoft.VCRedistVersion.default.txt') -Raw).Trim()
$base = Join-Path $vs "VC\Redist\MSVC\$version\x64"
$crt = Get-ChildItem -LiteralPath $base -Directory -Filter 'Microsoft.VC*.CRT' | Sort-Object Name | Select-Object -Last 1
if (-not $crt) { throw 'The Visual C++ runtime redistributable folder is missing.' }
$destination = Join-Path $PSScriptRoot '..\crates\leo-desktop\windows-runtime'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
Get-ChildItem -LiteralPath $crt.FullName -Filter '*.dll' -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $destination -Force }
foreach ($required in @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $destination $required))) { throw "The app-local runtime is missing $required." }
}
