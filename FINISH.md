# Slutför & verifiera bygget

> **Status:** Detta är ett komplett scaffold, skapat i en miljö **utan nätåtkomst och utan
> möjlighet att kompilera ML-beroendena** (whisper-rs, sherpa-rs, symphonia var inte cachade och
> modeller kunde inte laddas ner). Koden är skriven mot bibliotekens kända API:er men är **inte
> kompilerings-verifierad**. Slutför på en dev-maskin med öppet nät enligt nedan.

## 1. Förutsättningar (native bygg)

whisper.cpp och sherpa-onnx kompileras från C/C++:

- **Rust** (stable) + **CMake** + en **C/C++-kompilator**
  - Windows: Visual Studio Build Tools (C++ + Windows SDK)
  - macOS: Xcode Command Line Tools
  - Linux: `build-essential`, `cmake`, `clang`
- **Node.js** 18+
- **Python 3.x** (endast för att konvertera KB-BERT till ONNX)

## 2. Pinna och verifiera crate-API:er

De två känsligaste beroendena. Kontrollera senaste version och justera koden om signaturer skiljer:

```bash
cargo add whisper-rs           # uppdatera Cargo.toml till faktisk version
cargo add sherpa-rs
cargo doc -p whisper-rs -p sherpa-rs --open
```

**`transcribe.rs`** använder: `WhisperContext::new_with_params`, `create_state`, `FullParams`,
`state.full(...)`, `full_n_segments`, `full_get_segment_text/t0/t1`. Tidsstämplar antas vara i
centisekunder (10 ms). Verifiera mot din version.

**`diarize.rs`** antar `sherpa_rs::diarize::{Diarize, DiarizeConfig}` med
`Diarize::new(seg_model, emb_model, config)` och `compute(samples, progress) -> Segment{start,end,speaker}`.
Fält-/metodnamn kan skilja mellan versioner — justera vid behov.

## 3. Hämta modeller

```powershell
model-tools\fetch-whisper.ps1 -Size small   # → resources/whisper/kb-whisper-small.bin
model-tools\fetch-diarization.ps1           # → resources/diarization/segmentation.onnx + embedding.onnx
model-tools\build-pii-ner.ps1               # → resources/model/{model.onnx,tokenizer.json,labels.json}
model-tools\fetch-llm.ps1                   # → resources/llm/{model.gguf,tokenizer.json}
```

**Verifiera URL:erna** (markerade i skripten och i `src-tauri/src/models.rs`):

- **KB-Whisper GGML:** KBLab publicerar PyTorch/CT2-vikter. Om `ggml-model.bin` inte finns i
  HF-repot måste du konvertera med whisper.cpp (`models/convert-h5-to-ggml.py` + ev. `quantize`).
  Uppdatera då URL:erna i `WHISPER_MODELS` (`models.rs`) och `fetch-whisper.ps1`.
- **pyannote:** segmenteringsmodellen kräver ofta villkorsgodkännande på Hugging Face. sherpa-onnx
  publicerar färdiga ONNX-konverteringar i sina GitHub-releaser — kontrollera senaste filnamn.

## 4. Bygg & verifiera

```bash
npm install
cargo test --manifest-path src-tauri/Cargo.toml   # rena enhetstester (transcript, align)
npm run tauri dev                                  # kör appen
npm run tauri build                                # NSIS-installer
```

Manuell verifiering:
1. Välj en kort svensk ljudfil → Transkribera (utan diarisering).
2. Slå på diarisering med 2 talare → kontrollera "Talare 1/2"-uppdelning.
3. Avidentifiera → granska träffar → exportera .txt och .srt (råform + avidentifierad).

## 5. Kända risk-/TODO-punkter

- [ ] Verifiera whisper-rs- och sherpa-rs-API mot pinnade versioner (avsnitt 2). Detta inkluderar
      token-API:t för ordnivå-tidsstämplar (`full_n_tokens`, `full_get_token_text`,
      `full_get_token_data().t0/.t1`) och `WhisperContextParameters::use_gpu`.
- [ ] Bekräfta/ordna KB-Whisper GGML-filer + URL:er (avsnitt 3).
- [ ] pyannote-licens/villkor för distribution.
- [ ] Trimma diariserings-parametrar (`threshold`, `min_duration_*`) på riktigt material.
- [ ] **GPU-byggen**: installera CUDA Toolkit (NVIDIA) resp. ha Metal (macOS) / Vulkan SDK. Bygg med
      `--features cuda|metal|vulkan`. Verifiera att whisper-rs **och candle** exponerar dessa
      feature-namn i dina versioner (`candle-core`/`candle-transformers` har `cuda`/`metal`).
      `ai.rs::best_device()` antar `Device::new_cuda`/`new_metal` — stäm av mot candle-API:t.
      Diarisering (`sherpa-rs`) kör på CPU; lägg ev. till en GPU-feature där på samma sätt.
- [ ] **NER på GPU (valfritt):** KB-BERT via `ort` kör på CPU. För GPU, aktivera ORT:s CUDA/DirectML
      execution provider i `pii/model.rs` — separat API, ej kopplat till cargo-featuresen ovan.
- [ ] **Sammanfattningsmodeller:** verifiera GGUF/tokenizer-URL:erna i `models.rs` (SUMMARY_MODELS)
      och `fetch-summary.ps1`. Testa map-reduce på ett långt möte; trimma `CHUNK_CHARS` och
      `max_new` i `summarize.rs` efter vald modell. 7B på CPU är seg — rekommendera GPU-bygge.
      Överväg att strömma genererade tokens till UI:t (event) för långa sammanfattningar.
- [ ] **Markdown i docx:** `save_summary` skriver markdown-rubriker som literal text i .docx (v1).
      Överväg enkel rubrik-/punktlist-rendering om det behövs.
- [ ] **Progress-callback:** `transcribe.rs` använder `set_progress_callback_safe`. Verifiera namnet
      mot pinnad whisper-rs-version (äldre: `set_progress_callback`). Event: `avskrift:percent`.
- [ ] **Översättning:** `set_translate(true)` översätter till engelska. Whisper stöder bara
      engelska som målspråk — UI:t beskriver det så.
- [ ] **Projektfil (.avskrift):** JSON med transkript + talarnamn + ljudsökväg (ej inbäddat ljud).
      `audio_path` kan peka på en temp-WAV för inspelningar; överväg att kopiera ljud bredvid
      projektet om det ska vara flyttbart.
- [ ] **Synkad uppspelning:** kräver `security.assetProtocol` i `tauri.conf.json` (redan satt, scope
      `**`). Stäm av scope mot var ljudfiler/inspelningar faktiskt ligger om du snävar in det.
- [ ] **Mikrofon-behörighet**: macOS kräver `NSMicrophoneUsageDescription` i appens Info.plist
      (lägg i `tauri.conf.json` → `bundle.macOS.infoPlist` eller motsvarande). Windows (WebView2)
      visar en behörighetsfråga automatiskt.
- [ ] Stora ljudfiler/inspelningar: `save_recording` tar emot WAV-bytes via IPC (nedsamplat till
      16 kHz i webbläsaren). För mycket långa inspelningar, överväg att strömma till fil i stället.
- [ ] Stora ljudfiler: överväg chunkad transkribering + progress i procent (whisper progress-callback).
- [ ] Ikon/branding: `src-tauri/icons/` är kopierade från Avidentifierare — byt vid behov.

## 6. Lyfta ut till ett eget repo

Scaffoldet ligger i mappen `avskrift/` på grenen
`claude/transcription-diarization-tauri-vbytC` i avidentifierare-repot (miljön var låst till det
repot). Så här flyttar du det till ett färskt repo:

```bash
# Skapa ett nytt tomt repo på GitHub (t.ex. Pluggentipsar/avskrift), sedan lokalt:
cp -r avskrift /sökväg/till/avskrift && cd /sökväg/till/avskrift
git init -b main
git add .
git commit -m "Avskrift: transkribering, diarisering och avidentifiering (Tauri/Rust)"
git remote add origin git@github.com:Pluggentipsar/avskrift.git
git push -u origin main
```

(Alternativt: ge Claude åtkomst till det nya repot så kan jag pusha dit direkt.)
