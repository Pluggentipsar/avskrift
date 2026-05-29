# Hämtar diariserings-modellerna (ONNX) till resources/diarization/:
#   * segmentation.onnx  — pyannote segmentation 3.0 (sherpa-onnx-konverterad)
#   * embedding.onnx     — talar-embeddings (3D-Speaker / WeSpeaker / NeMo)
#
# Dessa är små och bäddas in i installern. Källor: k2-fsa/sherpa-onnx GitHub-releaser.
# OBS: Verifiera filnamn/versioner mot senaste sherpa-onnx-release.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $here "..\src-tauri\resources\diarization"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$ProgressPreference = "SilentlyContinue"

# Pyannote segmentation 3.0, konverterad till ONNX av sherpa-onnx-projektet.
$segUrl = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2"
# Talar-embedding (3D-Speaker, 16 kHz). Byt vid behov till wespeaker/nemo-modell.
$embUrl = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"

$tmp = Join-Path $env:TEMP "avskrift-seg.tar.bz2"
Write-Host "Hämtar segmenteringsmodell..."
Invoke-WebRequest -Uri $segUrl -OutFile $tmp
Write-Host "Packar upp (kräver tar)..."
tar -xjf $tmp -C $env:TEMP
Copy-Item (Join-Path $env:TEMP "sherpa-onnx-pyannote-segmentation-3-0\model.onnx") (Join-Path $dest "segmentation.onnx") -Force

Write-Host "Hämtar embedding-modell..."
Invoke-WebRequest -Uri $embUrl -OutFile (Join-Path $dest "embedding.onnx")

Write-Host "Klart. Filer i $dest"
Get-ChildItem $dest | Select-Object Name, @{n="MB";e={[math]::Round($_.Length/1MB,1)}}
