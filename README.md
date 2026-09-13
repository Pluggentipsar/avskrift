# Avskrift

Ett skrivbordsprogram som **transkriberar svenskt tal till text, skiljer talare åt (diarisering)
och avidentifierar** känsliga personuppgifter — allt **lokalt** på datorn. Inget ljud och ingen
text lämnar maskinen.

Programmet distribueras som färdiga Windows-paket med nödvändiga bibliotek och resurser.
Ingen Python eller molntjänst behövs för att använda appen.

> Syskonprojekt till [TystText/transav](https://github.com/Pluggentipsar/transav), men paketerat
> som en enbinärs Tauri/Rust-app i stället för Next.js + Python-backend.

## Funktioner

- **Mallflöde (0.7.0-beta.1, förhandsrelease)** — stöd för Supportärende och egna dokumentmallar,
  separata redigerbara utkast, källkopior och manuell AI-överlämning. Se
  [demoguide och avgränsningar](docs/MALLFLODE-MVP.md). Lokal modellkvalitet är ännu inte
  godkänd i de nya supportfallen; [testprotokollet](docs/demo-support/VERIFIERING.md) skiljer
  fungerande programflöde från återstående kvalitetsarbete. [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.7.0-beta.1).

- **Version 0.6.0 / arbetsyta 9** — ett sammanhängande mötesflöde med ljudtest, kanalval,
  anteckningar, beslut, åtgärder och uppföljning. Byt namn direkt i mötet, fäst eller arkivera
  arbeten och exportera ett samlat mötesunderlag. Svagt mikrofonljud hanteras bättre.
  Se [releasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.6.0)
  och [mötesguiden](docs/ARBETSYTA-STEG-9.md).

- **Arbetsyta 8** — lokalt sökindex, snabbare projektlistor och åtaganden, tydlig sökstatus
  och återuppbyggnad av biblioteket. Se [nyheter, tester och mätningar](docs/ARBETSYTA-STEG-8.md).

- **Arbetsyta 7** — strömmande ljudomvandling, avbrytbara förgrundsarbeten och företräde för
  diktering mellan modellsteg. Se [nyheter, tester och gränser](docs/ARBETSYTA-STEG-7.md).

- **Arbetsyta 6** — gemensam modellcache, automatisk GPU-budget, frigöring av inaktiva modeller
  och CPU-reservväg vid återhämtningsbara GPU-fel.
  Se [nyheter, tester och gränser](docs/ARBETSYTA-STEG-6.md).

- **Arbetsyta 5** — tokenbaserad uppdelning av långa AI-underlag, sammanställning i flera
  omgångar och tydligt förlopp med bevarade tidigare resultat vid fel.
  Se [nyheter, tester och gränser](docs/ARBETSYTA-STEG-5.md).

- **Arbetsyta 4** — samlad modellhantering, lugnare transkriptvy, sökning i hela underlaget,
  justerbar textstorlek och rendering av avsnitt nära läsytan för långa möten.
  Se [nyheter, tester och paket](docs/ARBETSYTA-STEG-4.md).

- **Arbetsyta 3** — källutkast för möten, diktatbearbetning med godkännande, original och
  maskerad text bredvid varandra samt egna mallar och granskningsprofiler.
  Se [nyheter och användning](docs/ARBETSYTA-STEG-3.md).

- **Arbetsyta 2** — original och återställbara versioner, sparade manuella maskningar,
  autosparad källtext, sparade diktat i biblioteket och import av Word-tabelltext.
  Se [nyheter, paket och begränsningar](docs/ARBETSYTA-STEG-2.md).

- **Diktera i andra program (Windows)** — håll Ctrl+Shift+Space och släpp för att transkribera,
  eller växla start/stopp med Ctrl+Alt+Space. Med lokal
  KB-Whisper och infogning i det fokuserade textfältet. Med indikator, sökbara diktat,
  kopiering och valfri sparad historik. Ljudet hålls i minnet. Se [Diktering](docs/DIKTERING.md).
- **Transkribering** med **KB-Whisper** (KBLab) — välj modellstorlek (tiny → large) efter dator och
  noggrannhetsbehov. Modeller hämtas vid behov; den minsta kan bäddas in i installern.
  Med **valbar GPU-acceleration** (CUDA / Metal / Vulkan) och **ordnivå-tidsstämplar**.
- **Inspelning** direkt i appen (mikrofon) — eller öppna en befintlig ljudfil.
- **Synkad uppspelning** — spela upp ljudet och följ med i transkriptet; klicka på ett ord eller
  yttrande för att hoppa dit. Med ordnivå-tidsstämplar markeras ordet som spelas.
- **Redigerbart transkript** — dubbelklicka på ett segment för att rätta ASR-fel; allt nedströms
  (avidentifiering, sammanfattning, export) använder den rättade texten. Plus **rättningsordlista**
  (fel⇒rätt på hela transkriptet) och **översättningsläge** (svenskt tal → engelsk text).
- **Spara/öppna projekt** — spara transkript, rättningar och talarnamn till en `.avskrift`-fil och
  återuppta senare, så långsam transkribering inte går förlorad.
- **Egen mall** — vid sammanfattning kan du klistra in din egen dagordning/rubriker.
- **Procent-progress** vid transkribering.
- **Diarisering** med **pyannote**-segmentering + talar-embeddings (via sherpa-onnx) — varje
  yttrande märks "Talare 1/2…", som du kan döpa om.
- **Avidentifiering** av transkriptet med samma motor som Avidentifierare:
  - **KB-BERT NER** — namn, platser, organisationer, tider
  - **Regler** — personnummer (Luhn), telefon, e-post, IP, ICD-10
  - **Ordlistor** — svenska diagnoser/mediciner + egen ordlista
  - **Valfritt AI-lager** — lokal Qwen2.5-1.5B (candle) för kontextuella ledtrådar
  - **Granskning** — varje träff godkänns/avvisas innan export; konsekvent pseudonymisering
- **Mötessammanfattning** — en valbar, nedladdningsbar lokal språkmodell (Qwen2.5 1,5B/3B/7B)
  sammanfattar transkriptet strukturerat enligt en **mall** (mötesprotokoll, kort sammanfattning,
  beslut & åtgärder). Långa möten hanteras via **map-reduce**. Resultatet är ett **redigerbart
  utkast** med "AI-genererat — granska"-varning; kan sammanfatta råtext eller den avidentifierade.
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
| AI-lager (PII) | `llama-cpp-2` (llama.cpp) | Qwen2.5-1.5B (GGUF) |
| Sammanfattning | `llama-cpp-2` (llama.cpp) | Qwen2.5 1,5B/3B/7B (GGUF, valbar) |
| Word-I/O | `docx-rs` | — |

## Bygga från källkod

> **Börja i [START.md](START.md)** — en steg-för-steg-guide som tar dig från klon till körande app
> i rätt ordning, och fångar fel tidigt. `model-tools/preflight.ps1` kollar verktygen åt dig och
> `model-tools/API-FIXES.md` är fusklappen om kompileringen klagar. (`FINISH.md` = djupare risklista.)

Kortversion:

```powershell
npm install

# Hämta/bygg modeller en gång (kräver öppet nät; Python bara för KB-BERT-konvertering):
model-tools\fetch-whisper.ps1 -Size small      # KB-Whisper (GGML)
model-tools\fetch-diarization.ps1              # pyannote + embedding (ONNX)
model-tools\build-pii-ner.ps1                  # KB-BERT -> int8 ONNX
model-tools\fetch-llm.ps1                      # Qwen2.5-1.5B (GGUF, PII-lager)
model-tools\fetch-summary.ps1 -Size 3b         # Qwen2.5-3B (GGUF, sammanfattning) – valfritt

npm run tauri dev      # utveckling

# Optimerade portabla Windows-byggen (se verktygskrav i docs/PRESTANDA.md):
npm run desktop:gpu -- -TargetDir C:\avb -LibClangPath C:\LLVM\bin
npm run desktop:cpu -- -TargetDir C:\avc -LibClangPath C:\LLVM\bin

# GPU-byggen — accelererar både Whisper (tal->text) och Qwen (AI-lagret):
npm run tauri build -- --features cuda     # NVIDIA  (Whisper + Qwen)
npm run tauri build -- --features metal    # Apple Silicon (Whisper + Qwen)
npm run tauri build -- --features vulkan   # plattformsoberoende GPU (Whisper + Qwen)
```

Windows-paketen hamnar i `dist/Avskrift-Vulkan` respektive `dist/Avskrift-CPU`.
Behåll hela paketmappen tillsammans. Se [prestanda och Windows-byggen](docs/PRESTANDA.md)
för bygginställningar, mätningar och skillnaden mellan första start och fortsatt diktering.

> GPU-byggena gäller **KB-Whisper** (via whisper.cpp) och **Qwen** (via llama.cpp). KB-BERT (NER via
> ONNX Runtime) kör alltid på CPU — det är redan snabbt och använder ett annat GPU-API.

## Modeller & licenser

- **KB-Whisper:** [KBLab](https://huggingface.co/KBLab) — se respektive modellkort
- **Diarisering:** pyannote segmentation 3.0 + talar-embedding (sherpa-onnx-konverteringar)
- **NER:** [KBLab/bert-base-swedish-cased-ner](https://huggingface.co/KBLab/bert-base-swedish-cased-ner)
- **AI-lager:** [Qwen2.5-1.5B-Instruct](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct) (Apache-2.0)

Kontrollera licensvillkoren för varje modell (särskilt pyannote, som kan kräva villkorsgodkännande
på Hugging Face) innan distribution.
