# Engångsbygge av modellresurserna. Kräver Python 3.x.
# Skapar en venv, installerar CPU-PyTorch + optimum, och kör konverteringen.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$venv = Join-Path $here ".venv"

if (-not (Test-Path $venv)) {
    Write-Host "Skapar venv..."
    python -m venv $venv
}

$py = Join-Path $venv "Scripts\python.exe"
& $py -m pip install --upgrade pip
& $py -m pip install torch --index-url https://download.pytorch.org/whl/cpu
& $py -m pip install "transformers" "optimum[onnxruntime]" "onnx" "onnxruntime"
& $py (Join-Path $here "convert.py")
