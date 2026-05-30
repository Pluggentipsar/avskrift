# Avskrift

Ett skrivbordsprogram som **transkriberar svenskt tal till text, skiljer talare åt (diarisering)
och avidentifierar** känsliga personuppgifter — allt **lokalt** på datorn. Inget ljud och ingen
text lämnar maskinen.

Byggt i samma anda som [Avidentifierare](../): en fil att installera, dubbelklicka och köra. Ingen
Python, ingen molntjänst, inga externa runtimes — allt bäddas in i binären.

> Syskonprojekt till [TystText/transav](https://github.com/Pluggentipsar/transav), men paketerat
> som en enbinärs Tauri/Rust-app i stället för Next.js + Python-backend.

## Funktioner

- **Transkribering** med **KB-Whisper** (KBLab) — välj modellstorlek (tiny → large) efter dator och
  noggrannhetsbehov. Modeller hämtas vid behov; den minsta kan bäddas in i installern.
  Med **valbar GPU-acceleration** (CUDA / Metal / Vulkan) och **ordnivå-tidsstämplar**.
- **Inspelning** direkt i appen (mikrofon) — eller öppna en befintlig ljudfil.
- **Diarisering** med **pyannote**-segmentering + talar-embeddings (via sherpa-onnx) — varje
  yttrande märks "Talare 1/2…", som du kan döpa om.
- **Avidentifiering** av transkriptet med samma motor som Avidentifierare:
  - **KB-BERT NER** — namn, platser, organisationer, tider
  - **Regler** — personnummer (Luhn), telefon, e-post, IP, ICD-10
  - **Ordlistor** — svenska diagnoser/mediciner + egen ordlista
  - **Valfritt AI-lager** — lokal Qwen2.5-1.5B (candle) för kontextuella ledtrådar
  - **Granskning** — varje träff godkänns/avvisas innan export; konsekvent pseudonymisering
- **Export**: ren text, Word (.docx), och undertexter **.srt / .vtt** med tidsstämplar — i råform
  eller avidentifierad. Med ordnivå-tidsstämplar även **ord-VTT** (en undertext per ord).

> Ingen automatik fångar 100 %. Granska alltid transkriptet och träffarna innan du delar.

## Teknik

Tauri 2 (Rust-backend) + SvelteKit (gränssnitt).

| Steg | Bibliotek | Modell |
|------|-----------|--------|
| Ljudavkodning → 16 kHz mono | `symphonia` + `rubato` | — |
| Tal → text | `whisper-rs` (whisper.cpp) | KB-Whisper (GGML) |
| Diarisering | `sherpa-rs` (sherpa-onnx) | pyannote-segmentering + talar-embedding (ONNX) |
| NER | `ort` (ONNX Runtime) | KB-BERT (int8 ONNX) |
| AI-lager | `candle` | Qwen2.5-1.5B (GGUF) |
| Word-I/O | `docx-rs` | — |

## Bygga från källkod

Se **[FINISH.md](FINISH.md)** för fullständig bygg- och verifieringsguide (inkl. modellhämtning,
versionspinning och NSIS-installer). Kortversion:

```powershell
npm install

# Hämta/bygg modeller en gång (kräver öppet nät; Python bara för KB-BERT-konvertering):
model-tools\fetch-whisper.ps1 -Size small      # KB-Whisper (GGML)
model-tools\fetch-diarization.ps1              # pyannote + embedding (ONNX)
model-tools\build-pii-ner.ps1                  # KB-BERT -> int8 ONNX
model-tools\fetch-llm.ps1                      # Qwen2.5-1.5B (GGUF)

npm run tauri dev      # utveckling
npm run tauri build    # NSIS-installer (CPU)

# GPU-byggen (whisper.cpp-backend):
npm run tauri build -- --features cuda     # NVIDIA
npm run tauri build -- --features metal    # Apple Silicon
npm run tauri build -- --features vulkan   # plattformsoberoende GPU
```

## Modeller & licenser

- **KB-Whisper:** [KBLab](https://huggingface.co/KBLab) — se respektive modellkort
- **Diarisering:** pyannote segmentation 3.0 + talar-embedding (sherpa-onnx-konverteringar)
- **NER:** [KBLab/bert-base-swedish-cased-ner](https://huggingface.co/KBLab/bert-base-swedish-cased-ner)
- **AI-lager:** [Qwen2.5-1.5B-Instruct](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct) (Apache-2.0)

Kontrollera licensvillkoren för varje modell (särskilt pyannote, som kan kräva villkorsgodkännande
på Hugging Face) innan distribution.
