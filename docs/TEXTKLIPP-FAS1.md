# Textklipp fas 1 – exakta ordtider

Status för [planens](TEXTKLIPP-PLAN.md) fas 1. Mätt lokalt 2026-10-02, RTX 5070 Ti,
Core Ultra 7 265KF, Windows.

## Klart

- **Ordgruppering rättad** (`transcribe.rs`, `group_words`): en token med bara mellanslag
  avslutar ordet. KB-Whisper large gav `" vad", " ", "AI"` → tidigare ordet `vadAI`.
  Tre nya enhetstester; ordinarie svit 110 passerade.
- **Modell:** `model-tools/export-voxrex.py` exporterar KBLab VoxRex (CC0-1.0, revision
  `ca70e31c…`) till ONNX med manifest och SHA-256.

  | Variant | Storlek | Ordtider mot PyTorch (425 ord, korta testfilmen) |
  | --- | --- | --- |
  | fp32 | 1 262 MB | identiska |
  | **fp16 (vald)** | 632 MB | 1 ordstart 80 ms, 2 ordslut ≤ 60 ms, övriga identiska |
  | int8 dynamisk | 317 MB | 43 ord > 40 ms fel, värst 4,9 s – **förkastad** |

- **`crates/wordalign`** (Rust, ort): normalisering (tal → svenska ord, "3,5" → "tre komma
  fem", %, &, främmande bokstäver), emissioner i fasta 22-sekundersfönster (20 s kärna + 1 s
  kontext per sida; en enda form för DirectML), CTC-Viterbi, justering i ~30-sekunderskörningar
  med 5 s marginal och upp till 40 efterföljande ord som kontext. Ord som inte går att uttala
  eller justera får interpolerad tid och `aligned: false`. Sex enhetstester.
- **GPU:** DirectML via `ort/directml`; appens medföljande `onnxruntime.dll` innehåller
  redan DML-providern. Misslyckad registrering är ett fel, ingen tyst CPU-reserv.

## Mätningar

Korta testfilmen (194,6 s), mot Python-referensen (global justering, PyTorch fp32):

| | Emissioner | Viterbi | Avvikelse > 40 ms |
| --- | --- | --- | --- |
| Rust CPU, fp16 | 20,7 s | 0,02 s | 3 ord (varav "12" nu korrekt som "tolv") |
| Rust DirectML, fp16 | 0,9 s | 0,01 s | 5 ord, i parti med låg modellsäkerhet |

Utan kontextord drev körningarnas sista ord in i nästa körnings tal (upp till 3,4 s);
med kontext försvann felet.

Egen inspelning, 1 945,8 s (32,4 min), 1080p50:

- KB-Whisper large (Vulkan): 58,9 s, 4 141 ord.
- Ordjustering DirectML: modell 1,1 s, emissioner 4,0 s, Viterbi 0,15 s. Alla ord
  justerade, i tidsordning; 5,5 % med poäng < 0,3.
- Andel ljudrutor med tal inom orden: Whisper 0,86, justerat 0,93.
- Whisper vs justerat, ordstart: median 0,30 s, p95 2,3 s, max 22,7 s. Största felet:
  Whisper lade inledningen ("Jag testar. Funkar detta?") på 93–110 s där det är tyst;
  talet börjar 115,5 s, där justeringen placerade orden.
- Mellanrum ≥ 150 ms mellan justerade ord: 953; 65 % innehåller minst 30 ms tystnad.
  Övriga klipps vid lägsta ljudnivå (se planen).

## Återstår i fas 1

1. Pausdetektering och klippunkter (tystaste bildrutegräns) i `crates/wordalign`.
2. `[ljud]`-block för tal utan ord (bortrensade eh/öh, otranskriberat).
3. Integrering i appen: modellhämtning (fp16, SHA-256) i modellinställningarna,
   justering efter transkribering, minnesregler i `memory.rs`, avbrytning via `work.rs`.
4. Manuell kontroll: 100 markerade ordgränser i tre inspelningar (mål median ≤ 30 ms).

```powershell
python model-tools/export-voxrex.py VOXREX_DIR OUT_DIR
cargo test --release --features directml --manifest-path crates/wordalign/Cargo.toml
cargo run --release --features directml --manifest-path crates/wordalign/Cargo.toml --example align -- `
  OUT_DIR/model.fp16.onnx OUT_DIR/vocab.json AUDIO16K.wav WHISPER.json OUT.json dml
```
