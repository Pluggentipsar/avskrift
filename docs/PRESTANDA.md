# Prestanda och Windows-byggen

Se också [granskningen av hela programmet](PRESTANDAGRANSKNING.md) för förbättringar av
mötesbearbetning, avidentifiering, AI-minne och historik samt kvarvarande större kandidater.

Transkribering av både filer och diktat använder samma motor. GPU-bygget aktiverar Vulkan
i Whisper och llama.cpp. Ett CPU-bygge använder inte grafikkortet, även om ett snabbt kort
är installerat. Dikteringsvyn visar vilket stöd bygget innehåller; det är inte en mätare
av vilket grafikkort som faktiskt används.

Modell och WhisperState behålls mellan körningar. Tidigare text används inte som prompt
för nästa inspelning. Vid modellbyte frigörs den gamla modellen före den nya laddas.
Diktering förbereder modellen i bakgrunden vid aktivering och appstart samt under inspelning
om motorn är ledig. Antalet CPU-trådar begränsas till högst åtta för att minska
synkroniseringskostnaden, särskilt på processorer med olika typer av kärnor.

## Lokal mätning, 2026-09-11

Dator: Intel Core Ultra 7 265KF, RTX 5070 Ti med 16 GB, NVIDIA-drivrutin 591.86.
Test: 11,65 sekunder syntetiskt svenskt tal, KB-Whisper tiny Q5, svenska valt, utan
ordtidsstämplar. Tid avser själva `Transcriber::transcribe`, inte mikrofonstopp,
gränssnitt eller infogning. Alla slutförda körningar gav 164 texttecken.

| Bygge | Varm modell, två körningar |
|---|---:|
| Tidigare native-bygginställningar, CPU, åtta trådar | 2,09 / 2,23 s |
| Optimerat native-bygge, CPU, åtta trådar | 0,306 / 0,293 s |
| Optimerat native-bygge, Vulkan på RTX 5070 Ti | 0,062 / 0,062 s |

Det tidigare försöket med 20 CPU-trådar avbröts efter mer än en minut utan färdig första
transkribering. Det är ingen färdig tidsmätning. CPU-raderna med åtta trådar går att jämföra.
Den optimerade CPU-mätningen kördes med GPU uttryckligen avstängd i samma Vulkan-kapabla
testbinär. Det mäter CPU-motorn, inte starttiden hos ett separat distribuerat CPU-paket.

Första GPU-körningen tog cirka 44 sekunder, inklusive shaderkompilering. Senare testprocesser
startade mycket snabbare med drivrutinens cache. Första start, drivrutinsbyte och cachetömning
kan därför ge längre väntan. Resultaten gäller detta korta test och denna dator; modellstorlek,
ljud, samtidiga jobb och andra datorer kan ge andra tider. Ingen noggrannhetsjämförelse gjordes.

## Reproducerbara portabla paket

Kräver Node/npm, Rust, Visual Studio C++ Build Tools med CMake/Ninja och libclang.
GPU-bygget kräver också Vulkan SDK och `VULKAN_SDK` satt till SDK-katalogen. Använd en kort,
separat byggkatalog för att undvika Windows sökvägsgräns.

```powershell
$env:VULKAN_SDK = 'C:\VulkanSDK\1.4.350.0' # anpassa till installerad version
npm run desktop:gpu -- -TargetDir C:\avb -LibClangPath C:\LLVM\bin
npm run desktop:cpu -- -TargetDir C:\avc -LibClangPath C:\LLVM\bin
```

Skriptet bygger gränssnittet och release-binären med inbäddat gränssnitt. Det använder explicit
Ninja, så CMake behåller `/O2` för C/C++-motorn. I det tidigare bygget hade cmake-rs ersatt
release-flaggorna utan `/O2`, trots att Rust byggdes med `--release`. En gammal olämplig
CMake-cache avvisas; välj då en ny byggkatalog.

Resultat: `dist/Avskrift-Vulkan/avskrift.exe` respektive `dist/Avskrift-CPU/avskrift.exe`.
Skriptet kopierar rätt llama/ggml-DLL:er från det aktuella bygget och övriga runtime-filer.
Flytta hela mappen tillsammans. Detta skapar ett portabelt paket, inte en installerare.
Modeller och appdata använder samma datakatalog som tidigare.

## Verifiering

`cargo test --release --features vulkan --lib` kör vanliga tester. Två opt-in-tester använder
en lokal WAV-fil och modell som anges i `AVSKRIFT_BENCH_AUDIO` och `AVSKRIFT_BENCH_MODEL`:

- `benchmark_transcription -- --ignored --nocapture`: tre körningar; skriver tider och
  textlängd, aldrig transkriberad text. `AVSKRIFT_BENCH_GPU=0` tvingar CPU och
  `AVSKRIFT_BENCH_THREADS` ändrar trådantal för jämförelse.
- `cached_state_keeps_recordings_independent -- --ignored`: samma ljud före och efter ett
  annat ljud med andra ordtidsinställningar ska ge samma text.

Använd samma optimerade byggmiljö och `--target-dir` som paketet. Mät utan andra samtidiga
transkriberings- eller byggjobb. Kontrollera native-loggen för den faktiska GPU-enheten.
