$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'chewton2k/leo-cli'
$Target = 'x86_64-pc-windows-msvc'
$BinDir = if ($env:LEO_INSTALL_DIR) { $env:LEO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\leo' }

function Say([string]$Text) { Write-Host $Text }
function Step([string]$Text) { Write-Host "  ok " -ForegroundColor Green -NoNewline; Write-Host $Text }
function Doing([string]$Text) { Write-Host "  >  " -ForegroundColor Cyan -NoNewline; Write-Host $Text }
function Note([string]$Text) { Write-Host "  !  " -ForegroundColor Yellow -NoNewline; Write-Host $Text }
function Fail([string]$Text) {
    Write-Host "  x  $Text" -ForegroundColor Red
    Write-Host "  Nothing was changed. Help: https://github.com/$Repo/issues"
    exit 1
}
function Pretty([string]$Path) {
    if ($Path.StartsWith($HOME)) { return '~' + $Path.Substring($HOME.Length) }
    return $Path
}
function Sha256([string]$Path) { (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLower() }
function Fetch([string]$Url, [string]$Dest) {
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        & $curl.Source -fL --progress-bar -o $Dest $Url
        return ($LASTEXITCODE -eq 0)
    }
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing
        return $true
    } catch {
        return $false
    }
}

Say ''
Say '  leo installer'
Say '  notes in your terminal, with AI and recording'
Say ''

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64') {
    Fail "there is no ready-made build for Windows on $arch yet. Build it from source instead: https://github.com/$Repo#1-install-leo"
}
Step 'Found Windows on x86-64'

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("leo-install-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $archive = Join-Path $tmp 'leo.zip'
    if ($env:LEO_INSTALL_ARCHIVE) {
        Copy-Item $env:LEO_INSTALL_ARCHIVE $archive
        Step ("Using " + (Split-Path $env:LEO_INSTALL_ARCHIVE -Leaf))
    } else {
        $url = "https://github.com/$Repo/releases/latest/download/leo-$Target.zip"
        Doing 'Downloading leo'
        if (-not (Fetch $url $archive)) { Fail "could not download $url" }
        $sumFile = Join-Path $tmp 'leo.zip.sha256'
        if (Fetch "$url.sha256" $sumFile) {
            $expected = ((Get-Content $sumFile -Raw).Trim() -split '\s+')[0].ToLower()
            if ($expected -eq (Sha256 $archive)) {
                Step 'Checksum verified'
            } else {
                Fail 'the download is damaged (checksum mismatch); try again'
            }
        }
    }

    $unpacked = Join-Path $tmp 'unpacked'
    Expand-Archive -Path $archive -DestinationPath $unpacked -Force
    $newExe = Join-Path $unpacked 'leo.exe'
    if (-not (Test-Path $newExe)) { Fail 'the download does not contain leo.exe' }

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    $dest = Join-Path $BinDir 'leo.exe'
    $previous = ''
    if (Test-Path $dest) {
        try { $previous = ((& $dest --version) -split ' ')[1] } catch { $previous = '' }
        $parked = "$dest.old"
        Remove-Item $parked -Force -ErrorAction SilentlyContinue
        Move-Item $dest $parked -Force
    }
    Copy-Item $newExe $dest -Force
    Remove-Item "$dest.old" -Force -ErrorAction SilentlyContinue
    Step ("Installed to " + (Pretty $dest))
    $version = ''
    try { $version = ((& $dest --version) -split ' ')[1] } catch { $version = '' }

    $models = if ($env:LEO_HOME) { Join-Path $env:LEO_HOME 'models' } else { Join-Path $HOME '.leo\models' }
    $modelDir = Join-Path $models 'parakeet-tdt-0.6b-v3-int8'
    $modelUrl = if ($env:LEO_INSTALL_MODEL_URL) { $env:LEO_INSTALL_MODEL_URL } else { 'https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78' }
    $manifest = if ($env:LEO_INSTALL_MODEL_MANIFEST) { $env:LEO_INSTALL_MODEL_MANIFEST } else { 'encoder.int8.onnx=acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247 decoder.int8.onnx=179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e joiner.int8.onnx=3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3 tokens.txt=d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d' }
    if (-not $env:LEO_INSTALL_NO_MODEL) {
        $need = @()
        $damaged = $false
        foreach ($pair in ($manifest -split '\s+' | Where-Object { $_ })) {
            $name, $sha = $pair -split '=', 2
            $file = Join-Path $modelDir $name
            if (-not (Test-Path $file)) {
                $need += ,@($name, $sha)
            } elseif ((Sha256 $file) -ne $sha.ToLower()) {
                $need += ,@($name, $sha)
                $damaged = $true
            }
        }
        $ready = $false
        if ($need.Count -eq 0) {
            $ready = $true
            Step ("Speech model ready in " + (Pretty $modelDir))
        } else {
            if ($damaged) {
                Doing 'The speech model is damaged; downloading it again'
            } else {
                Doing 'Downloading the speech model (Parakeet, 670 MB, once)'
            }
            New-Item -ItemType Directory -Path $modelDir -Force | Out-Null
            $ready = $true
            foreach ($item in $need) {
                $name = $item[0]
                $sha = $item[1].ToLower()
                $part = Join-Path $modelDir "$name.part"
                if ((Fetch "$modelUrl/$name" $part) -and ((Sha256 $part) -eq $sha)) {
                    Move-Item $part (Join-Path $modelDir $name) -Force
                } else {
                    Remove-Item $part -Force -ErrorAction SilentlyContinue
                    $ready = $false
                    break
                }
            }
            if ($ready) {
                Step ("Speech model saved to " + (Pretty $modelDir))
            } else {
                Note 'Could not download the speech model; leo downloads it when it starts'
            }
        }
        Remove-Item (Join-Path $models 'ggml-base.en.bin') -Force -ErrorAction SilentlyContinue
    }

    if (-not $env:LEO_INSTALL_SKIP_PATH) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @($userPath -split ';' | Where-Object { $_ })
        $present = $entries | Where-Object { $_.TrimEnd('\') -ieq $BinDir.TrimEnd('\') }
        if ($present) {
            Step 'Already on your PATH'
        } else {
            [Environment]::SetEnvironmentVariable('Path', (($entries + $BinDir) -join ';'), 'User')
            Step ("Added " + (Pretty $BinDir) + " to your PATH")
        }
    }
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Say ''
Say '  ----------------------------------------------------'
Say ''
if (-not $previous) {
    Say "  leo $version is installed. Thank you for trying it!"
} elseif ($previous -eq $version) {
    Say "  leo is already up to date ($version). Thank you for using it!"
} else {
    Say "  leo is updated: $previous -> $version. Thank you for using it!"
}
Say ''
Say '  Get started'
Say '    leo            open your notes'
Say '    leo doctor     check AI, recording and backup, and store an API key'
Say ''
Say "  Guide: https://github.com/$Repo#readme"
if (-not $env:LEO_INSTALL_SKIP_PATH) {
    Say ''
    Say '  Open a new terminal first, so it sees the new PATH.'
}
Say ''
