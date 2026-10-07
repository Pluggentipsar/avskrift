# Avskrift

Ett skrivbordsprogram för Windows som gör svenskt tal till text du kan arbeta vidare med:
möten, intervjuer, diktat och egna videoinspelningar. Allt körs **lokalt** på datorn. Inget ljud
och ingen text lämnar maskinen, och ingen Python eller molntjänst behövs.

> Syskonprojekt till [TystText/transav](https://github.com/Pluggentipsar/transav), men paketerat
> som en enbinärs Tauri/Rust-app i stället för Next.js + Python-backend.

## Vad Avskrift gör

- **Möten** – spela in digitala möten med din mikrofon och mötesljudet som två spår, med text
  i realtid om du vill. Efter mötet finns texten direkt; det som saknas kompletteras i
  bakgrunden. Anteckningar och tidsmarkeringar under mötet, beslut och åtgärder med ansvarig och
  datum, uppföljning och ett samlat mötesunderlag att exportera.
- **Transkribera** – ljud- och videofiler med KB-Whisper (tiny till large) eller den snabbare
  Pianissimo (experimentell, svenska). Talare skiljs åt, uppspelningen följer texten ord för
  ord, och texten går att rätta direkt. Även översättning till engelsk text.
- **Diktering** – håll Ctrl+Shift+Space (eller växla med Ctrl+Alt+Space) i vilket program som
  helst; texten skrivs in där markören står. Se [Diktering](docs/DIKTERING.md).
- **Textklipp** – klipp en egen videoinspelning genom att stryka text i transkriptet och
  exportera en färdig film med exakt synk och undertexter. Se [Textklipp](docs/TEXTKLIPP.md).
- **Avidentifiering** – namn, platser, personnummer, telefonnummer med mera hittas med en
  svensk NER-modell, regler och ordlistor, och granskas träff för träff innan en maskerad kopia
  delas.
- **Sammanfatta och skapa från mall** – en lokal språkmodell gör redigerbara utkast:
  sammanfattningar, protokoll och egna dokumentmallar. Du kan också ställa frågor om källan.
  Utkasten markeras som AI-genererade och ska granskas.
- **Bibliotek och åtaganden** – allt sparas och söks lokalt, med mappar, fästa och arkiverade
  arbeten och en samlad lista över åtaganden.
- **Export** – text, Word (.docx) och undertexter (.srt, .vtt), i original eller avidentifierat.

> Ingen automatik fångar allt. Granska transkript, maskningar och utkast innan du delar dem.

## Hämta

Ladda ner från [releaserna](https://github.com/Pluggentipsar/avskrift/releases). Senaste
förhandsversion är [0.8.0-beta.4](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.8.0-beta.4);
senaste stabila är [0.6.0](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.6.0).

| Fil | För |
| --- | --- |
| `Avskrift_<version>_x64-setup-vulkan.exe` | Datorer med grafikkort (NVIDIA, AMD, Intel med Vulkan). Snabbast. |
| `Avskrift_<version>_x64-setup-cpu.exe` | Datorer utan lämpligt grafikkort. |
| `Avskrift-<version>-Windows-*.zip` | Samma sak utan installation (portabel mapp). |

Välj installationsprogrammet om du kan: från 0.8.0-beta.3 uppdaterar sig appen själv via
**Sök efter uppdatering** i menyn (signerade uppdateringar, bara när du ber om det; se
[Uppdateringar](docs/UPPDATERINGAR.md)). Talmodeller och språkmodeller hämtas i appen under
**Modeller på datorn** när du behöver dem.

Programmen är inte kodsignerade, så Windows SmartScreen kan varna. Välj *Mer information* och
*Kör ändå*. Vad som kommit i varje version: [Versioner](docs/VERSIONER.md).

## Teknik

Tauri 2 (Rust-backend) och SvelteKit (gränssnitt). Allt körs i appens egen process på datorn.

| Steg | Bibliotek | Modell |
| --- | --- | --- |
| Ljudavkodning → 16 kHz mono | `symphonia`, `rubato` | – |
| Tal → text | `whisper-rs` (whisper.cpp, Vulkan) | KB-Whisper (GGML) |
| Tal → text, alternativ | `ort` (ONNX Runtime, CPU) | Pianissimo (Klangs ONNX-export, int8) |
| Talarseparering | `sherpa-rs` (sherpa-onnx) | pyannote-segmentering och talarembedding (ONNX) |
| Exakta ordtider (Textklipp) | `ort` (DirectML) | KBLab wav2vec2 VoxRex (ONNX, fp16) |
| Video (Textklipp) | FFmpeg 8.1 (LGPL, medföljer) | – |
| Personuppgifter, NER | `ort` (ONNX Runtime) | KB-BERT (int8 ONNX) |
| Personuppgifter, AI-lager | `llama-cpp-2` (llama.cpp) | Qwen2.5-1.5B (GGUF) |
| Sammanfattning, mallar, frågor | `llama-cpp-2` (llama.cpp) | Qwen2.5 1,5B, 3B eller 7B (GGUF, valbar) |
| Word | `docx-rs` | – |
| Uppdateringar | `tauri-plugin-updater` | – |

## Bygga från källkod

> **Börja i [START.md](START.md)**: en steg-för-steg-guide från klon till körande app.
> `model-tools/preflight.ps1` kollar verktygen och `model-tools/API-FIXES.md` hjälper om
> kompileringen klagar.

```powershell
npm install

# Modeller som följer med appen (en gång; Python behövs bara för KB-BERT-konverteringen):
model-tools\fetch-diarization.ps1              # pyannote + talarembedding (ONNX)
model-tools\build-pii-ner.ps1                  # KB-BERT -> int8 ONNX
model-tools\fetch-llm.ps1                      # Qwen2.5-1.5B (GGUF, AI-lagret)
model-tools\fetch-ffmpeg.ps1                   # LGPL-FFmpeg för Textklipp

npm run tauri dev                              # utveckling
npm run tauri build -- --features vulkan       # GPU-bygge (Whisper och Qwen via Vulkan)
```

En hel Windows-release (båda varianterna, signerade installationsprogram, ZIP-filer och
uppdateringsfiler) byggs med `model-tools\release\release.ps1`; se
[Uppdateringar](docs/UPPDATERINGAR.md). Bygginställningar och mätningar finns i
[Prestanda](docs/PRESTANDA.md). KB-BERT och Pianissimo körs alltid på processorn.

## Modeller och licenser

- **KB-Whisper:** [KBLab](https://huggingface.co/KBLab), se respektive modellkort
- **Pianissimo:** [Klang AI](https://huggingface.co/KlangAI/pianissimo-sv), CC BY 4.0
- **Ordtider:** [KBLab/wav2vec2-large-voxrex-swedish](https://huggingface.co/KBLab/wav2vec2-large-voxrex-swedish), CC0 1.0
- **NER:** [KBLab/bert-base-swedish-cased-ner](https://huggingface.co/KBLab/bert-base-swedish-cased-ner)
- **Språkmodeller:** [Qwen2.5-Instruct](https://huggingface.co/Qwen) 1,5B/3B/7B, Apache-2.0
- **Talarseparering:** pyannote segmentation 3.0 och talarembedding (sherpa-onnx-konverteringar)
- **FFmpeg:** LGPL version 3 eller senare, medföljer som separata bibliotek

Se [NOTICE](NOTICE.md). Kontrollera licensvillkoren för varje modell innan vidare distribution.
