param([string]$OutputDir = (Join-Path $PSScriptRoot '../.build-tools/pianissimo/model'))
$ErrorActionPreference = 'Stop'
# Community ONNX export; pinned independently of the original NeMo checkpoint.
$revision = '72c38267654dadd538bceac7a851de00fb55f11a'
$base = "https://huggingface.co/moonhouse/pianissimo-sv-onnx/resolve/$revision"
$files = @(
    @{ Name = 'encoder-model.int8.onnx'; Hash = '8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2' },
    @{ Name = 'decoder_joint-model.int8.onnx'; Hash = '2fb4ef1c1e28839aef70e74a3a2737afdc7460afa1e640ce1c4f9bf9ceadcb51' },
    @{ Name = 'tokenizer.model'; Hash = 'eacec2b0a77f336d4a2ca4a25a7047575d3c2b74de47e997f4c205126ed3135e' },
    @{ Name = 'vocab.txt' }, @{ Name = 'config.json' }, @{ Name = 'README.md' }
)
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
foreach ($file in $files) {
    $target = Join-Path $OutputDir $file.Name
    if ((Test-Path -LiteralPath $target) -and $file.Hash -and
        (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq $file.Hash) { continue }
    Write-Host "Hämtar $($file.Name)…"
    $partial = "$target.partial"
    & curl.exe --fail --location --retry 3 --silent --show-error --output $partial "$base/$($file.Name)"
    if ($LASTEXITCODE -ne 0) { throw "Nedladdningen misslyckades: $($file.Name)" }
    if ($file.Hash -and (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $file.Hash) {
        throw "Fel kontrollsumma: $($file.Name). Den ofullständiga filen har inte aktiverats."
    }
    Move-Item -LiteralPath $partial -Destination $target -Force
}
@{ repository = 'moonhouse/pianissimo-sv-onnx'; revision = $revision;
   original = 'KlangAI/pianissimo-sv'; license = 'CC-BY-4.0' } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDir 'source.json') -Encoding utf8
Write-Host "Klart: $([IO.Path]::GetFullPath($OutputDir))"
