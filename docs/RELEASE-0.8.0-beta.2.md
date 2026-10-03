# Avskrift 0.8.0-beta.2 – Pianissimo från Klangs egen export

Förhandsrelease som byter Pianissimo till Klangs egen ONNX-export. Allt från 0.8.0-beta.1
(Textklipp, ny arbetsyta, egen ingång för Transkribera) ingår oförändrat. Version 0.6.0 ligger
kvar som senaste stabila release.

## Nytt

- **Pianissimo från Klangs egen export** (int8 med SmoothQuant). Uppmätt i Avskrift, med WER
  räknat som på Klangs modellkort:
  - Långa inspelningar (30 minuter sammanhängande tal): **5,6 %** fel mot 6,6 % tidigare.
  - Korta klipp (FLEURS, 759 klipp): 6,6 % mot 6,5 %, i praktiken lika.
  - **45–50 gånger snabbare än realtid** på processorn, mot 5–10 gånger tidigare.
- **Ingen förberedelse:** modellen (660 MB i stället för 923 MB) kan användas direkt efter
  hämtningen. Tidigare tog första starten flera minuter och krävde 4,5 GB ledigt minne.
- **Längre avsnitt:** långa inspelningar körs i avsnitt om 110 sekunder i stället för 28,
  med färre skarvar.

## Om du redan har hämtat Pianissimo

Pianissimo visas som ej hämtad efter uppdateringen. Öppna **Modeller på datorn** och välj
**Hämta modell** igen. Bara de ändrade filerna hämtas, och den gamla förberedda kopian
(cirka 1 GB) tas bort. KB-Whisper och dina arbeten påverkas inte.

## Hämta och installera

- **Avskrift_0.8.0-beta.2_x64-setup-vulkan.exe:** installationsprogram, GPU-version för dator
  med kompatibelt grafikkort. Whisper och de lokala språkmodellerna körs på grafikkortet.
- **Avskrift_0.8.0-beta.2_x64-setup-cpu.exe:** installationsprogram för dator utan
  Vulkan-stöd eller när CPU-körning önskas.
- **Windows-Vulkan.zip / Windows-CPU.zip:** samma sak som portabla mappar utan installation.
  Packa upp hela ZIP-filen till en ny mapp och starta `avskrift.exe`.

Pianissimo körs på processorn i båda versionerna.

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar. Dina arbeten ligger
kvar i appens datamapp. Modellerna hämtas vid behov i appen och ingår inte i paketen.
Installationsprogrammen är inte kodsignerade, så Windows SmartScreen kan varna. Välj
*Mer information* och *Kör ändå*.

## Verifierat och återstående

- Pianissimo mätt i Avskrifts egen Rust-motor på FLEURS svenska testmängd (759 klipp) och på
  tre sammanfogade inspelningar om 10 minuter (147 meningar). Resultatet för korta klipp
  stämmer med Klangs egen siffra (6,56 % mot angivna 6,62 %).
- 112 ordinarie Rust-tester passerade på vardera CPU och Vulkan; Pianissimo-motorns 7
  enhetstester passerade. Appens installationstest och hela transkriberingsvägen (avkodning,
  avbrytning, talartilldelning, SRT/VTT) passerade mot riktiga modellfiler.
- Svelte/TypeScript: 0 fel, 3 tidigare varningar.
- Båda versionerna byggda med optimerad whisper.cpp (Ninja, /O2). Varje exe- och DLL-fil
  kontrollerades mot sina beroenden; inga saknades. CPU-versionen har inget Vulkan-beroende.
- Inte mätt: riktiga möten med flera talare och brus. Installationsprogrammen är inte
  provinstallerade i detta bygge.
- Känt fel, inte nytt i denna version: talarseparationen kan i sällsynta fall krascha appen
  på långa inspelningar (sett i test på en 10 minuters fil). Det utreds separat.

Se `docs/PIANISSIMO.md` för mätningar och begränsningar. SHA256SUMS.txt innehåller
kontrollsummor för alla filer i releasen.
