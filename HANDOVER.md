# Överlämning — fortsätt här (andra datorn)

> Skapad 2026-05-31. Snabb lägesbild så du kommer igång direkt på nästa maskin.
> Djupare bygg-detaljer finns i [START.md](START.md), [API-FIXES.md](model-tools/API-FIXES.md)
> och [FINISH.md](FINISH.md).

## TL;DR — var vi är

Hela **START.md steg 1–6 är klara och verifierade** på den första datorn:
appen **bygger, länkar, testar och startar** med riktiga modeller. Det enda som återstår är
**steg 7 — manuell genomgång i UI:t** (klicka runt, transkribera, avidentifiera, sammanfatta).

| Steg | Status |
|------|--------|
| 1 — Frontend (`npm run build`) | ✅ grönt |
| 2 — `cargo check` (hela backenden) | ✅ 0 fel |
| 3 — API-fixar (whisper-rs/sherpa-rs/tauri) | ✅ gjorda (se nedan) |
| 4 — `cargo test` | ✅ 34 passed, 0 failed, 5 ignored (smoke-tester som kräver modeller) |
| 5 — Modeller hämtade | ✅ alla 6 (lokalt, **ej** i git — se nedan) |
| 6 — `npm run tauri dev` | ✅ kör i **release** (måste vara release på Windows, se nedan) |
| 7 — Manuell genomgång | ⏳ kvar — din tur i appen |

## Sätt upp nästa dator (i ordning)

1. **Klona repot** och installera verktyg. Kör `model-tools/preflight.ps1`
   (kollar Rust, Node, CMake, MSVC). Tre saker preflight INTE fångar — fixa dem:
   - **MSVC-env:** `cl.exe` ligger inte i PATH; importera via
     `…\VC\Auxiliary\Build\vcvars64.bat` (eller bygg i "x64 Native Tools"-prompten).
   - **CMake:** VS har en inbyggd i
     `…\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin` — lägg på PATH.
   - **libclang (bindgen):** krävs av whisper-rs-sys/sherpa-rs-sys/ort-sys. Saknades; löstes
     **utan admin** med `python -m pip install --user libclang`, sätt sedan
     `LIBCLANG_PATH` till `…\site-packages\clang\native`. (Alternativt installera LLVM, men
     det kräver admin/UAC.)
2. **Frontend:** `npm install` (om rollup-bugg om saknat native-paket: ta bort
   `node_modules` + `package-lock.json` och kör `npm install` igen) → `npm run build`.
3. **Hämta modellerna igen** (de ligger INTE i git — se "Vad som INTE är i git"). Kör i
   `model-tools/`:
   ```powershell
   model-tools\build-pii-ner.ps1                 # KB-BERT -> ONNX (bygger egen .venv; kräver Python)
   model-tools\fetch-llm.ps1                      # Qwen2.5-1.5B (PII-lager)
   model-tools\fetch-diarization.ps1             # pyannote + embedding (kräver tar; finns i Win11)
   model-tools\fetch-whisper.ps1 -Size small     # KB-Whisper
   model-tools\fetch-summary.ps1 -Size 3b        # Qwen2.5-3B (sammanfattning)
   ```
   Alla fem käll-URL:er var live (200 OK) 2026-05-31. Whisper kan också hämtas inifrån appen.
4. **Kör appen i RELEASE** (viktigt, se nedan):
   ```powershell
   npm run tauri dev -- --release
   ```

## ⚠️ Windows-fälla: bygg i RELEASE, inte debug

En vanlig `npm run tauri dev` (debug) öppnar fönstret men **kraschar i samma sekund som
whisper.cpp laddar modellen** — debug-CRT-assertion `_osfile(fh) & FOPEN` i `read.cpp:381`,
sedan `STATUS_STACK_BUFFER_OVERRUN (0xc0000409)`. Orsak: Rust länkar release-CRT (`/MD`) på
Windows, men i debug bygger `cmake`-craten whisper.cpp/sherpa med debug-CRT (`/MDd`);
whisper.cpp:s C-`fread` på GGML-filen korsar de oförenliga runtimes. **Lösning: bygg
release** (`-- --release`, eller `npm run tauri build`) → CMake använder `/MD` och matchar.
Release kör dessutom inferensen mycket snabbare.

## Vad som ändrades (committat) — de riktiga API-fixarna

Koden var skriven offline och aldrig kompilerad. Driften mot crate-API:erna var liten men
verklig:

- **`src-tauri/Cargo.toml`** — `tauri`-craten behöver `features = ["protocol-asset"]`
  (eftersom `tauri.conf.json` slår på `security.assetProtocol` för synkad ljuduppspelning).
- **`src-tauri/src/diarize.rs`** — sherpa-rs 0.6.8: `DiarizeConfig`-fälten är `Option<f32>`
  (wrappa `threshold`/`min_duration_*` i `Some(..)`); `Diarize::new` är generisk
  `AsRef<Path>` (skicka `&Path` direkt); `compute(samples, None)` — progress-callbacken är
  `Option<Box<dyn Fn(i32,i32)->i32>>`, inte en bar closure.
- **`src-tauri/src/transcribe.rs`** — whisper-rs 0.14.4: `full_get_token_data()` returnerar
  `Result<WhisperTokenData,_>` — matcha den innan `.t0`/`.t1`. Allt annat stämde.
- `pii/model.rs` (ort 2.0.0-rc.12) och `ai.rs`/`summarize.rs` (candle 0.10.2) behövde
  **inga** ändringar.

11 cargo-varningar kvar — alla "dead code"/oanvänt, ofarliga.

## Vad som INTE är i git (måste återskapas lokalt)

Allt detta är `.gitignore`:at — totalt ~3,4 GB lokalt:
- `src-tauri/resources/{whisper,diarization,llm,model,summary-models}/` — **alla modeller**
  (hämtas med skripten i steg 3 ovan).
- `model-tools/.venv/` — Python-venv för KB-BERT-konvertering (byggs om av `build-pii-ner.ps1`).
- `src-tauri/target/`, `node_modules/`, `build/`, `.svelte-kit/` — byggartefakter.
- Obs: jag la **placeholder.txt** i `resources/{model,llm,diarization}/` på datorn för att
  klara ett bygg-glob i `tauri.conf.json` — de är gitignore:ade och behövs inte när du har
  riktiga modeller (skripten skriver dit dem ändå).

## Modeller som finns (på dator 1, för referens)

| Modell | Fil | Storlek |
|--------|-----|---------|
| KB-Whisper small | `resources/whisper/kb-whisper-small.bin` | 487 MB |
| Diarisering | `resources/diarization/segmentation.onnx` + `embedding.onnx` | 6 + 40 MB |
| LLM 1.5B (PII) | `resources/llm/model.gguf` + `tokenizer.json` | 986 + 7 MB |
| PII-NER (KB-BERT int8) | `resources/model/model.onnx` + `tokenizer.json` + `labels.json` | 125 MB |
| Sammanfattning 3B | `resources/summary-models/qwen2.5-3b.gguf` + `.tokenizer.json` | 1,93 GB |

## Steg 7 — kvar att testa (checklista)

Kort svensk ljudfil räcker. Bocka av i appen:
- [ ] Transkribera fil → text + procent-progress
- [ ] Spela in via mikrofon → transkribera
- [ ] Diarisering 2 talare → "Talare 1/2"; döp om
- [ ] Uppspelning: klicka tidsstämpel → ljudet hoppar dit
- [ ] Ordnivå (slå på före transkribering) → ord markeras ett och ett
- [ ] Redigera segment (dubbelklick) → spara
- [ ] Rättningsordlista `fel=>rätt` → tillämpa
- [ ] Avidentifiera → granska → exportera `.txt`/`.docx`
- [ ] Sammanfatta → "Medel (3B)" + "Egen mall"
- [ ] Export: tidsstämplar på/av, `.srt`/`.vtt`, kombinerad
- [ ] Spara projekt → stäng → Öppna projekt
- [ ] Översättningsläge → svenskt tal → engelsk text

## Senare / kvar enligt FINISH.md
- GPU-byggen (`-- --features cuda|metal|vulkan`) — kräver CUDA Toolkit resp. Metal/Vulkan-SDK.
- Verifiera KB-Whisper GGML- och pyannote-licenser inför distribution.
- Trimma diariserings-parametrar på riktigt material.
- (Mac) `NSMicrophoneUsageDescription` i Info.plist för mikrofon.
