# API-FIXES — slå upp när `cargo check` (steg 2) felar

Koden skrevs mot bibliotekens *kända* API:er men kunde inte kompileras i skaparmiljön. Nedan listas
varje osäkert antagande: **fil:rad**, vad koden antar, och hur du rättar mot den faktiska versionen.

Slå upp aktuella signaturer:

```powershell
cargo doc --manifest-path src-tauri/Cargo.toml -p whisper-rs -p sherpa-rs -p candle-core --open
```

Arbeta uppifrån och ned, kör `cargo check` mellan varje fix. Oftast är det bara några metodnamn.

---

## 1. whisper-rs (störst risk) — `src-tauri/src/transcribe.rs`

Pinnad mot `whisper-rs = "0.14"`. Kontrollera mot din faktiska version (`cargo tree -p whisper-rs`).

| Rad | Antagande | Om det felar |
|-----|-----------|--------------|
| ~52 | `WhisperContext::new_with_params(path, params)` | Äldre: `WhisperContext::new(path)`. Nyare kan kräva `&WhisperContextParameters`. |
| ~51 | `WhisperContextParameters::use_gpu(true)` | Kan heta `.use_gpu = true` (fält) eller saknas (GPU via feature only). Ta då bort raden. |
| ~92 | `params.set_progress_callback_safe(|p: i32| …)` | Äldre: `set_progress_callback`. Om ingen säker variant finns: ta bort raden (progress-baren blir då vilande — appen funkar ändå). |
| ~84 | `params.set_translate(bool)` | Stabilt; bör finnas. |
| ~89 | `params.set_token_timestamps(bool)` | Kan heta `set_token_timestamps` / `token_timestamps`. Krävs för ordnivå. |
| ~115 | `state.full_n_tokens(i)` | Stabilt. |
| ~117 | `state.full_get_token_text(i, j)` | Stabilt. |
| ~124 | `state.full_get_token_data(i, j)` → `.t0 / .t1` | Verifiera fältnamn (`WhisperTokenData`). t0/t1 är i centisekunder. |

**Om progress-callbacken inte går att lösa:** ta bort `set_progress_callback_safe`-raden och ändra
`run_transcription` i `lib.rs` så `avskrift:percent` inte emittas. Allt annat fungerar; bara
procentbaren tystnar (textstatus finns kvar).

---

## 2. sherpa-rs (näst störst risk) — `src-tauri/src/diarize.rs`

Pinnad mot `sherpa-rs = "0.6"`. Diariserings-API:t varierar mellan versioner.

| Rad | Antagande | Om det felar |
|-----|-----------|--------------|
| ~31 | `use sherpa_rs::diarize::{Diarize, DiarizeConfig}` | Modulen kan heta `speaker_diarization`. Sök i `cargo doc`. |
| ~40 | `DiarizeConfig { num_clusters, threshold, min_duration_on, min_duration_off, .. }` | Fältnamn kan skilja (t.ex. `clustering`/`segmentation`-understrukturer). Bygg configen efter doc:en. |
| ~49 | `Diarize::new(seg_path, emb_path, config)` | Konstruktorns argumentordning/typ kan skilja; vissa tar en samlad config med modellsökvägar. |
| ~58 | `sd.compute(samples, |done,total| …)` | Kan heta `process`; callbacken kan saknas. |
| ~66 | resultat-fält `s.start`, `s.end`, `s.speaker` | Verifiera fältnamn (kan vara `.start_time` / `.label`). |

Diarisering är fristående: får du inte ihop den snabbt, kommentera bort anropet i `lib.rs`
(`run_transcription`) tillfälligt och testa resten — transkribering/avident/sammanfattning är
oberoende av den.

---

## 3. candle GPU — `src-tauri/src/ai.rs`

| Rad | Antagande | Om det felar |
|-----|-----------|--------------|
| ~52 | `Device::new_cuda(0)` | Stabilt i candle 0.10. Endast med `--features cuda`. |
| ~58 | `Device::new_metal(0)` | Stabilt i candle 0.10. Endast med `--features metal`. |

Detta kompileras bara in när motsvarande feature är på. Bygg först helt utan `--features` (CPU);
då rörs inte dessa rader.

---

## 4. Modell-URL:er (vid 404 i steg 5)

### KB-Whisper GGML — `src-tauri/src/models.rs` (~rad 37–46) + `fetch-whisper.ps1`
Antar `https://huggingface.co/KBLab/kb-whisper-<size>/resolve/main/ggml-model.bin`.
**KBLab kan publicera PyTorch/CTranslate2 i stället för GGML.** Om filen saknas:
1. Hämta KB-Whisper-vikterna och konvertera med whisper.cpp:
   `python models/convert-h5-to-ggml.py` (+ ev. `quantize`), eller
2. Leta upp en färdig GGML-konvertering och uppdatera URL:erna i **både** `models.rs`
   (`WHISPER_MODELS`) och `fetch-whisper.ps1`.

### Diarisering — `fetch-diarization.ps1`
ONNX-modeller från `k2-fsa/sherpa-onnx`-releaser. Filnamn/versioner ändras — kontrollera senaste
release och uppdatera de två URL:erna. pyannote-segmentering kan kräva villkorsgodkännande på HF.

### Sammanfattning/PII — `fetch-summary.ps1` / `fetch-llm.ps1`
Qwen2.5 GGUF via `bartowski` + tokenizer från `Qwen`. Stabila men dubbelkolla att kvantfilen
(`Q4_K_M`) finns kvar med samma namn.

---

## 5. Övrigt

- **NER (`ort`)**: kör CPU. Inget att fixa för att bygga.
- **Asset-protokoll (uppspelning)**: kräver `security.assetProtocol` i `tauri.conf.json` (redan satt).
- **`set_progress_callback_safe` + trådar**: callbacken körs på whisper-tråden; vi emittar bara ett
  Tauri-event, vilket är trådsäkert.

När `cargo check` (steg 2) och `cargo test` (steg 4) är gröna är de farliga bitarna avklarade —
resten är nedladdning och manuell test.
