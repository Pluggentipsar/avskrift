# Arkitektur

Avskrift är en enbinärs Tauri-app. All tung beräkning sker i Rust-backenden; SvelteKit-frontenden
är bara gränssnitt. Pipelinen är:

```
ljudfil
  │  audio.rs        symphonia-avkodning + rubato-resampling → 16 kHz mono f32
  ▼
samples ─┬─ transcribe.rs   whisper-rs (KB-Whisper GGML) → RawSegment[]  (text + tider)
         │
         └─ diarize.rs      sherpa-rs (pyannote + embedding) → SpeakerTurn[]  (tider + kluster)
                   │
                   ▼
            align.rs         tilldela varje segment dominerande talare → Utterance[]
                   │
                   ▼
            transcript.rs    Transcript-modell + export (txt / srt / vtt / docx)
                   │
                   ▼  (valfritt)
            engine.rs        PII-detektion per yttrande → granskning → maskerad text
            pii/ + ai.rs     KB-BERT (ort) · regler · ordlistor · Qwen (candle)
```

## Rust-moduler (`src-tauri/src/`)

| Modul | Ansvar |
|-------|--------|
| `lib.rs` | Tauri-kommandon, delad `Backend`-state, orkestrering |
| `models.rs` | Modellkatalog, sökvägsupplösning (bundlade resurser + skrivbar app-data-dir) |
| `download.rs` | Nedladdning av Whisper-modeller med progress (ureq, atomiskt via `.part`) |
| `audio.rs` | Avkodning/nedmixning/resampling till 16 kHz mono f32 |
| `transcribe.rs` | whisper.cpp via `whisper-rs`; lat laddning, modellbyte |
| `diarize.rs` | sherpa-onnx-diarisering → talar-turer |
| `align.rs` | Slår ihop transkript + diarisering, stabil "TALARE_n"-märkning |
| `transcript.rs` | Datamodell + export-format (txt/srt/vtt/docx) |
| `engine.rs` | **Återanvänd** PII-motor; `analyze_segments`/`anonymized_segments` tillagda |
| `pii/`, `ai.rs`, `docio.rs`, `data/` | **Återanvänt oförändrat** från Avidentifierare |

## Tauri-kommandon

- `list_whisper_models() -> WhisperModelInfo[]`
- `download_whisper_model(id)` → event `avskrift:download {id, downloaded, total}`
- `transcribe(args{path, model, language, diarize, num_speakers}) -> Transcript`
- `anonymize(args{texts, enabled, terms, use_ai}) -> AnalyzeResult`
- `anonymized_segments(rejected) -> string[]`
- `export_transcript(args{path, anonymize, rejected, speaker_labels})`

Framstegsmeddelanden sänds som event `avskrift:progress`. Tung körning sker på
`tauri::async_runtime::spawn_blocking` så UI:t inte fryser.

## Designval

- **Ren Rust/ONNX, ingen Python** — bevarar Avidentifierares enklicks-/enbinärsinstallation.
  whisper.cpp + sherpa-onnx kör hela tal-pipen via samma typ av inbäddad runtime som NER redan
  använder (`ort`).
- **Whisper-modeller hämtas vid behov** (flera storlekar = för stort att bädda in alla). Små
  modeller (diarisering, NER) och en förvald Whisper kan bäddas in i installern.
- **Avidentifiering per yttrande** — varje yttrande är ett "stycke" så pseudonymer blir konsekventa
  i hela transkriptet och maskerad text kan återbyggas per yttrande för srt/vtt/docx med bevarade
  tidsstämplar och talarnamn.

## Återanvändning av Avidentifierare

`pii/`, `ai.rs`, `docio.rs` och `data/` är kopierade oförändrade. `engine.rs` är identisk så när som
på två tillagda metoder (`analyze_segments`, `anonymized_segments`). Förbättringar i en av
kodbaserna kan därför enkelt portas till den andra.
