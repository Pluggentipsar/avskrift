param([string]$OutputDir = (Join-Path $PSScriptRoot '../src-tauri/ffmpeg'))
# LGPL v3 FFmpeg for Textklipp (BtbN FFmpeg-Builds, shared libraries, no x264/x265).
# Pinned to a dated autobuild; the zip is verified before anything is unpacked. FFmpeg 8.1, not 9.0:
# 9.0's NVENC needs NVIDIA driver >= 610 (RTX 5070 Ti here had 591.86 and fell back to Media Foundation).
# Only ffmpeg.exe, ffprobe.exe, their DLLs and the licence text are kept (no ffplay/headers/docs).
$ErrorActionPreference = 'Stop'
$tag = 'autobuild-2026-10-01-13-06'
$name = 'ffmpeg-n8.1.3-14-g330caae0c1-win64-lgpl-shared-8.1'
$hash = 'bf545d8fee9bb6957c1f3dea0f384bf64edead407d763326dbbd2de1b04768a4'
$url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$tag/$name.zip"
$work = Join-Path $PSScriptRoot '../.build-tools/ffmpeg-lgpl'
New-Item -ItemType Directory -Path $work, $OutputDir -Force | Out-Null
$zip = Join-Path $work "$name.zip"
if (!(Test-Path -LiteralPath $zip) -or (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $hash) {
    Write-Host "Hämtar $name…"
    $partial = "$zip.partial"
    & curl.exe --fail --location --retry 3 --silent --show-error --output $partial $url
    if ($LASTEXITCODE -ne 0) { throw "Nedladdningen misslyckades: $url" }
    if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $hash) {
        throw 'Fel kontrollsumma för FFmpeg. Den ofullständiga filen har inte aktiverats.'
    }
    Move-Item -LiteralPath $partial -Destination $zip -Force
}
$unpacked = Join-Path $work 'x'
if (Test-Path -LiteralPath $unpacked) { Remove-Item -LiteralPath $unpacked -Recurse -Force }
Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
$root = Join-Path $unpacked $name
Get-ChildItem -LiteralPath $OutputDir -File | Remove-Item -Force
Copy-Item -LiteralPath (Join-Path $root 'bin/ffmpeg.exe'), (Join-Path $root 'bin/ffprobe.exe') -Destination $OutputDir
Get-ChildItem -LiteralPath (Join-Path $root 'bin') -Filter '*.dll' | Copy-Item -Destination $OutputDir
Copy-Item -LiteralPath (Join-Path $root 'LICENSE.txt') -Destination (Join-Path $OutputDir 'LICENSE-FFmpeg.txt')
@{ source = 'https://github.com/BtbN/FFmpeg-Builds'; tag = $tag; asset = "$name.zip"; sha256 = $hash
   ffmpeg = 'n8.1.3-14-g330caae0c1'; license = 'LGPL-3.0-or-later' } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDir 'source.json') -Encoding utf8
Write-Host "Klart: $([IO.Path]::GetFullPath($OutputDir))"
