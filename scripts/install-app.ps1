$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Repo = 'chewton2k/leo-cli'
$Target = 'x86_64-pc-windows-msvc'
if ($env:OS -ne 'Windows_NT') { throw 'Leo is available for macOS and Windows only.' }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') { throw 'Download Leo for Windows on x64.' }
$AppDir = if ($env:LEO_WINDOWS_APP_DIR) { $env:LEO_WINDOWS_APP_DIR } else { Join-Path $env:LOCALAPPDATA 'Leo' }
$BinDir = if ($env:LEO_INSTALL_DIR) { $env:LEO_INSTALL_DIR } else { Join-Path $AppDir 'bin' }
foreach ($path in @($AppDir, $BinDir)) {
    if (-not [IO.Path]::IsPathRooted($path) -or $path -match '["\r\n]') { throw 'Choose absolute installation folders without quotes or line breaks.' }
}
if ($env:LEO_UPDATE_PARENT_PID) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (Get-Process -Id ([int]$env:LEO_UPDATE_PARENT_PID) -ErrorAction SilentlyContinue) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Close the terminal Leo command and try the installer again.' }
        Start-Sleep -Milliseconds 100
    }
}
$running = Get-Process -Name 'leo-app' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.Equals((Join-Path $AppDir 'leo-app.exe'), [StringComparison]::OrdinalIgnoreCase) }
if ($running) { throw 'Quit Leo before installing. Finish any recording or dictation first.' }
function Fetch([string]$Url, [string]$Destination) {
    & curl.exe --fail --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 900 --max-filesize 1073741824 --retry 4 --retry-delay 3 --output $Destination $Url
    if ($LASTEXITCODE -ne 0) { throw 'The Leo download failed. Check your connection and try again.' }
}
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('leo-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $installer = Join-Path $tmp 'leo-setup.exe'
    if ($env:LEO_INSTALL_ARCHIVE) { Copy-Item -LiteralPath $env:LEO_INSTALL_ARCHIVE -Destination $installer }
    else {
        $effective = & curl.exe --fail --silent --location --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 60 --retry 4 --retry-delay 3 --output NUL --write-out '%{url_effective}' "https://github.com/$Repo/releases/latest"
        if ($LASTEXITCODE -ne 0 -or $effective -notmatch '/tag/(v\d+\.\d+\.\d+)$') { throw 'The latest Leo release could not be identified. Try again later.' }
        $tag = $Matches[1]
        Write-Host "Downloading Leo $tag..."
        $url = "https://github.com/$Repo/releases/download/$tag/leo-$Target-setup.exe"
        Fetch $url $installer
        $checksum = Join-Path $tmp 'checksum'
        Fetch "$url.sha256" $checksum
        $expected = ((Get-Content -LiteralPath $checksum -Raw).Trim() -split '\s+')[0]
        if ($expected -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash) { throw 'The app download is damaged. Download it again.' }
    }
    if ((Get-Item -LiteralPath $installer).Length -gt 1073741824) { throw 'The installer is too large. Download it again.' }
    foreach ($path in @($AppDir, $BinDir)) {
        if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The installation folder is a link. Choose another folder.' }
    }
    Write-Host 'Installing Leo. This can take a minute...'
    $process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$AppDir") -PassThru
    if (-not $process.WaitForExit(900000)) { throw 'The installer is still running. Wait for it to finish before trying again.' }
    if ($process.ExitCode -ne 0) { throw 'The app could not be installed. Reopen the downloaded installer and try again.' }
    $cli = Join-Path $AppDir 'leo.exe'
    if (-not (Test-Path -LiteralPath $cli) -or -not (Test-Path -LiteralPath (Join-Path $AppDir 'leo-app.exe'))) { throw 'The installer did not provide the Leo app and terminal command.' }
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    $link = Join-Path $BinDir 'leo.cmd'
    $content = "@setlocal`r`n@for /f `"tokens=2 delims=:`" %%a in ('chcp') do @set `"LEO_OLD_CODEPAGE=%%a`"`r`n@chcp 65001 >nul`r`n" + '@"' + $cli.Replace('%', '%%') + '" %*' + "`r`n@set `"LEO_COMMAND_STATUS=%errorlevel%`"`r`n@chcp %LEO_OLD_CODEPAGE% >nul`r`n@exit /b %LEO_COMMAND_STATUS%`r`n"
    $linkTemp = Join-Path $BinDir (".leo-link-" + [Guid]::NewGuid().ToString('N'))
    [IO.File]::WriteAllText($linkTemp, $content, [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $linkTemp -Destination $link -Force
    $old = Join-Path $BinDir 'leo.exe'
    if (-not $old.Equals($cli, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $old)) {
        try { Remove-Item -LiteralPath $old -Force }
        catch { Move-Item -LiteralPath $old -Destination (Join-Path $BinDir (".leo-old-" + [Guid]::NewGuid().ToString('N') + '.exe')) -Force }
    }
    Get-ChildItem -LiteralPath $BinDir -Filter '.leo-old-*.exe' -Force -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    $config = if ($env:LEO_HOME) { $env:LEO_HOME } else { Join-Path $env:APPDATA 'leo' }
    New-Item -ItemType Directory -Path $config -Force | Out-Null
    $record = Join-Path $config 'app-cli-dir'
    if ((Test-Path -LiteralPath $record) -and ((Get-Item -LiteralPath $record).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Remove the app-cli-dir link from the Leo settings folder before installing.' }
    $recordTemp = Join-Path $config (".app-cli-dir-" + [Guid]::NewGuid().ToString('N'))
    [IO.File]::WriteAllText($recordTemp, $BinDir, [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $recordTemp -Destination $record -Force
    if (-not $env:LEO_INSTALL_SKIP_PATH) {
        $current = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @($current -split ';' | Where-Object { $_ })
        if (-not ($entries | Where-Object { $_.TrimEnd('\').Equals($BinDir.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) })) {
            [Environment]::SetEnvironmentVariable('Path', (($entries + $BinDir) -join ';'), 'User')
        }
        if (-not (@($env:Path -split ';') | Where-Object { $_.TrimEnd('\').Equals($BinDir.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) })) { $env:Path = "$env:Path;$BinDir" }
    }
    Write-Host "Leo is installed in $AppDir. Open Leo from Start, or type leo here or in any new terminal."
    Write-Host 'Your notes stay where they are. Thank you for using Leo!'
} finally { Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue }
