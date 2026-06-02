# Hämtar den lokala LLM-modellen (Qwen2.5-1.5B-Instruct, Q4_K_M) + tokenizer till
# src-tauri/resources/llm/. Körs en gång efter klon. Slutanvändaren behöver inte detta —
# modellen bäddas in i den färdiga installern.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $here "..\src-tauri\resources\llm"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$ProgressPreference = "SilentlyContinue"

Write-Host "Hämtar tokenizer..."
Invoke-WebRequest -Uri "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/resolve/main/tokenizer.json" `
    -OutFile (Join-Path $dest "tokenizer.json")

Write-Host "Hämtar modell (~940 MB)..."
Invoke-WebRequest -Uri "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q8_0.gguf" `
    -OutFile (Join-Path $dest "model.gguf")

Write-Host "Klart. Filer i $dest"
Get-ChildItem $dest | Select-Object Name, Length
