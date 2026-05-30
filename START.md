# START HÄR — bygg och verifiera Avskrift

Den här guiden tar dig från nyklonat repo till en körande app, i rätt ordning. Den är gjord för att
**fånga fel tidigt och billigt**: vi kompilerar koden (utan att ladda ner modeller) innan vi lägger
tid på gigabytes nedladdning.

> Bakgrund: koden skapades i en miljö utan nät och är **inte kompilerings-verifierad**. De mest
> osäkra punkterna är API:erna i `whisper-rs` och `sherpa-rs` — steg 2 nedan hittar dem åt dig.

Plattform: stegen visar **Windows / PowerShell** (projektets primära mål). Mac/Linux: se noterna.

---

## Översikt — sju steg

| # | Steg | Nät? | Tid |
|---|------|------|-----|
| 0 | Verktyg på plats (`preflight.ps1`) | nej | 5 min |
| 1 | `npm install` + frontend-bygge | ja | 5 min |
| 2 | **`cargo check`** — kompilera allt utan modeller | ja (crates) | 10–30 min* |
| 3 | Fixa ev. API-fel (se `model-tools/API-FIXES.md`) | — | varierar |
| 4 | `cargo test` — logiktester | nej | 2 min |
| 5 | Hämta modeller | ja (stort) | 10–40 min |
| 6 | `npm run tauri dev` — kör appen | nej | — |
| 7 | Manuell genomgång (checklistan sist) | nej | 10 min |

\* Första gången kompileras whisper.cpp/sherpa-onnx från C++ — det tar tid men sker bara en gång.

---

## Steg 0 — Verktyg

Kör preflight-skriptet. Det kollar allt som behövs och säger exakt vad som ev. saknas:

```powershell
cd avskrift
model-tools\preflight.ps1
```

Det verifierar: **Rust**, **Cargo**, **Node 18+**, **npm**, **CMake**, och en **C++-kompilator**
(MSVC). Om något saknas skriver det ut var du hämtar det. Installera och kör om tills allt är grönt.

> **Varför C++/CMake?** `whisper-rs` och `sherpa-rs` bygger C/C++-bibliotek (whisper.cpp,
> sherpa-onnx) från källkod. Utan kompilator faller steg 2.

Mac: `xcode-select --install`, `brew install cmake node rust`.
Linux: `sudo apt install build-essential cmake`, samt Node + Rust.

---

## Steg 1 — Frontend

```powershell
npm install
npm run build      # bygger SvelteKit -> ../build
```

Detta är snabbt och har inget med ML att göra. Om det felar är det ett rent JS/Svelte-problem
(lättfixat). När det är grönt vet vi att gränssnittet är OK.

---

## Steg 2 — Kompilera Rust UTAN modeller  ← viktigast

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Detta kompilerar **hela** backenden — inklusive `whisper-rs` och `sherpa-rs` — men kör ingenting och
laddar inga modeller. Det är här eventuella API-fel dyker upp, t.ex.:

- `set_progress_callback_safe` finns inte i din whisper-rs-version
- `WhisperContextParameters::use_gpu` heter annorlunda
- `sherpa_rs::diarize`-typerna ser annorlunda ut

**Gå inte vidare förrän detta är grönt.** Felmeddelandena pekar på fil och rad. Slå upp varje fel i
nästa steg.

> Första körningen laddar ned och kompilerar många crates + C++ — ha tålamod. Senare körningar är
> sekundsnabba.

---

## Steg 3 — Fixa API-fel (om steg 2 felade)

Öppna **`model-tools/API-FIXES.md`**. Den listar varje osäkert API-antagande, vilken fil/rad det
sitter i, och hur du rättar det mot den faktiska versionen. Slå upp aktuell signatur med:

```powershell
cargo doc --manifest-path src-tauri/Cargo.toml -p whisper-rs -p sherpa-rs --open
```

Rätta, kör `cargo check` igen, upprepa tills grönt. (Oftast handlar det om 1–3 metodnamn.)

---

## Steg 4 — Logiktester (inga modeller)

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

Kör de rena enhetstesterna (transkript-format, talar-tilldelning, chunkning, PII-regler). Snabbt och
modell-fritt. Ska vara grönt.

---

## Steg 5 — Hämta modeller

Nu — och först nu — laddar vi ner modeller. Kör de fyra skripten. **Verifiera URL:erna** om något
404:ar (se `API-FIXES.md`, avsnittet "Modell-URL:er").

```powershell
model-tools\build-pii-ner.ps1                 # KB-BERT -> int8 ONNX  (kräver Python)
model-tools\fetch-llm.ps1                      # Qwen2.5-1.5B (PII-lager)
model-tools\fetch-diarization.ps1             # pyannote + embedding (ONNX)
model-tools\fetch-whisper.ps1 -Size small     # KB-Whisper (GGML)
model-tools\fetch-summary.ps1 -Size 3b        # Qwen2.5-3B (sammanfattning) — valfritt
```

Whisper- och sammanfattningsmodeller kan även hämtas **inifrån appen** senare, så `fetch-whisper`/
`fetch-summary` är frivilliga här om du hellre testar nedladdnings-UI:t direkt.

Minsta uppsättning för att appen ska starta och kunna transkribera:
**build-pii-ner + fetch-llm + fetch-diarization + minst en Whisper-modell.**

---

## Steg 6 — Kör appen

```powershell
npm run tauri dev
```

Första bygget länkar in C++-biblioteken — det tar någon minut. Sedan öppnas fönstret.

GPU-bygge (valfritt, först när CPU-vägen funkar):

```powershell
npm run tauri dev -- --features cuda     # NVIDIA   (Whisper + Qwen)
npm run tauri dev -- --features metal    # Apple    (Whisper + Qwen)
npm run tauri dev -- --features vulkan   # GPU      (endast Whisper)
```

När allt fungerar — bygg installern:

```powershell
npm run tauri build      # -> src-tauri/target/release/bundle/nsis/Avskrift_x.y.z_x64-setup.exe
```

---

## Steg 7 — Manuell genomgång

Bocka av i tur och ordning (kort svensk testfil räcker):

- [ ] **Transkribera** en ljudfil → text + procent-progress rör sig.
- [ ] **Spela in** via mikrofon → transkribera inspelningen.
- [ ] **Diarisering** med 2 talare → "Talare 1/2" delas upp; döp om en talare.
- [ ] **Uppspelning**: klicka på en tidsstämpel → ljudet hoppar dit; markering följer.
- [ ] **Ordnivå** (slå på före transkribering) → ord markeras ett och ett; `.vtt (ord)` syns.
- [ ] **Redigera** ett segment (dubbelklick) → rätta → spara.
- [ ] **Rättningsordlista**: `fel=>rätt` → tillämpa → rätt ord byts i hela transkriptet.
- [ ] **Avidentifiera** → granska träffar → exportera `.txt`/`.docx`.
- [ ] **Sammanfatta** (hämta 3B först) → välj mall, även "Egen mall" → utkast visas och går att redigera.
- [ ] **Export**: tidsstämplar på/av, kombinerad (protokoll + transkript), `.srt`/`.vtt`.
- [ ] **Spara projekt** → stäng → **Öppna projekt** → allt återställs.
- [ ] **Översättningsläge** → svenskt tal blir engelsk text.

Klart! Hittar du något som inte stämmer — notera vilket steg, så är det lätt att åtgärda riktat.

---

## Om något krånglar

- **Steg 2 felar med okänd metod/typ** → `model-tools/API-FIXES.md`.
- **Modell 404** → URL:er i `src-tauri/src/models.rs` + fetch-skripten; se `API-FIXES.md`.
- **Länkfel om CUDA/Metal** → bygg först utan `--features` (CPU). GPU kräver CUDA Toolkit resp.
  Metal/Vulkan-SDK.
- **Mikrofon nekas (Mac)** → lägg `NSMicrophoneUsageDescription` i Info.plist (se `FINISH.md`).
- **Djupare teknisk bakgrund** → `ARCHITECTURE.md`. **Fullständig TODO/risklista** → `FINISH.md`.
