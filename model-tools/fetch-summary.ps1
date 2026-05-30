# Hämtar en sammanfattningsmodell (Qwen2.5-Instruct, GGUF Q4_K_M) + tokenizer till
# src-tauri/resources/summary-models/<id>.gguf och <id>.tokenizer.json.
#
# Slutanvändaren behöver INTE detta — appen hämtar modeller på begäran via UI:t. Detta är för att
# bygga en installer med en förvald sammanfattningsmodell, eller för utveckling.
#
# OBS: Verifiera URL:erna mot models.rs (SUMMARY_MODELS).
param(
    [ValidateSet("1.5b", "3b", "7b")]
    [string]$Size = "3b"
)
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $here "..\src-tauri\resources\summary-models"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$ProgressPreference = "SilentlyContinue"

switch ($Size) {
    "1.5b" { $id = "qwen2.5-1.5b"; $repo = "Qwen2.5-1.5B-Instruct" }
    "3b"   { $id = "qwen2.5-3b";   $repo = "Qwen2.5-3B-Instruct" }
    "7b"   { $id = "qwen2.5-7b";   $repo = "Qwen2.5-7B-Instruct" }
}
$gguf = "https://huggingface.co/bartowski/$repo-GGUF/resolve/main/$repo-Q4_K_M.gguf"
$tok  = "https://huggingface.co/Qwen/$repo/resolve/main/tokenizer.json"

Write-Host "Hämtar tokenizer ($id)..."
Invoke-WebRequest -Uri $tok -OutFile (Join-Path $dest "$id.tokenizer.json")
Write-Host "Hämtar modell ($id, Q4_K_M)..."
Invoke-WebRequest -Uri $gguf -OutFile (Join-Path $dest "$id.gguf")

Write-Host "Klart. Filer i $dest"
Get-ChildItem $dest -Filter "$id*" | Select-Object Name, @{n="MB";e={[math]::Round($_.Length/1MB)}}
