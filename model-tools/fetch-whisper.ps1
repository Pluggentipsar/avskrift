# Hämtar KB-Whisper-modeller i GGML-format (whisper.cpp) till resources/whisper/<id>.bin.
#
# Slutanvändaren behöver INTE detta — appen kan hämta modeller på begäran. Detta är för att
# bygga en installer med en förvald modell inbäddad, eller för utveckling.
#
# OBS: Verifiera GGML-URL:erna nedan mot faktiskt publicerade artefakter. KBLab publicerar
# PyTorch/CTranslate2-vikter; GGML-filer kommer från en konvertering (egen eller community).
# Konvertera vid behov med whisper.cpp: `models/convert-h5-to-ggml.py` eller `quantize`.
param(
    [ValidateSet("tiny", "base", "small", "medium", "large")]
    [string]$Size = "small"
)
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $here "..\src-tauri\resources\whisper"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$ProgressPreference = "SilentlyContinue"

$url = "https://huggingface.co/KBLab/kb-whisper-$Size/resolve/main/ggml-model.bin"
$out = Join-Path $dest "kb-whisper-$Size.bin"

Write-Host "Hämtar kb-whisper-$Size ..."
Invoke-WebRequest -Uri $url -OutFile $out
Write-Host "Klart: $out"
Get-ChildItem $out | Select-Object Name, @{n="MB";e={[math]::Round($_.Length/1MB)}}
