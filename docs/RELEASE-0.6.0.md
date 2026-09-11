# Avskrift 0.6.0

En samlad arbetsyta för möten, lokal diktering och avidentifiering, med bättre prestanda för både CPU och GPU.

## Nyheter

- **Hela mötet på ett ställe:** ljudtest och val av mikrofon/mötesljud, agenda, deltagare, live-text, tidsmarkerade anteckningar, beslut och åtgärder. Efter stopp fortsätter du i samma möte medan transkriptet bearbetas.
- **Enklare att organisera:** byt namn direkt vid mötesrubriken, fäst och arkivera arbeten, fortsätt där du slutade och förbered uppföljningsmöten med valda öppna åtgärder.
- **Bättre mikrofontranskribering:** svagt mikrofonljud förstärks inför transkribering, ofullständig live-text kan återhämtas från originalspåren och ett textfilter som kunde radera riktiga repliker har tagits bort. Originalinspelningen bevaras.
- **Diktera i andra program:** håll **Ctrl+Shift+Space** och släpp för att transkribera. **Ctrl+Alt+Space** växlar start/stopp utan att du behöver hålla tangenterna inne. Valfri lokal historik gör det lätt att återanvända text.
- **Granska och exportera:** källbelagda AI-utkast, återställbara versioner, original och avidentifierad text sida vid sida, egna mallar och valbara delar i mötesexport till text eller Word.
- **Bättre flyt på olika datorer:** återanvändning av laddade modeller, automatisk GPU-minnesbudget, frigöring av inaktiva modeller, CPU-reservväg vid återhämtningsbara GPU-fel och avbrytbara arbeten.
- **Långa underlag och stora bibliotek:** strömmande ljudomvandling, rendering av transkriptavsnitt nära läsytan, lokalt sökindex och tokenbaserad uppdelning av långa AI-underlag.

## Hämta och starta

Windows x64. Paketen i denna release är **portabla ZIP-filer**:

- **Avskrift-0.6.0-Windows-Vulkan.zip** — GPU-version för datorer med Vulkan-stöd, exempelvis NVIDIA RTX 5070 Ti.
- **Avskrift-0.6.0-Windows-CPU.zip** — version för CPU utan krav på Vulkan.

Packa upp hela ZIP-filen, stäng den gamla appen även i aktivitetsfältets meddelandefält och starta **avskrift.exe** i den nya mappen. Ingen ominstallation behövs. Behåll DLL-filerna och mappen `resources` tillsammans med programmet. Befintliga arbeten och hämtade modeller finns kvar i appens användardata. Tal- och språkmodeller hämtas vid behov från appen.

**LÄS-MIG.md** och mötesguiden finns i paketen. **SHA256SUMS.txt** innehåller kontrollsummor för ZIP-filerna.

## Verifiering och kvarvarande gränser

Inför releasen har 101 ordinarie Rust-tester passerat på både CPU och Vulkan. Ett separat test med svagt syntetiskt tal och riktig KB-Whisper-small har också passerat på båda. UI-regressioner omfattar bland annat sparning, källgranskning, avbrytning, bibliotek och mötesflödet. Svelte/TypeScript-kontrollen har inga fel; tre tidigare varningar återstår.

Olika fysiska headset och enhetsbyten behöver fortsatt praktisk provning. Fixen för mikrofontext är verifierad med syntetiskt tal; orsaken i en viss äldre inspelning kan variera. Öppna ett drabbat möte och välj **Kör om med vald modell**, med ekoborttagning avstängd till att börja med. Ett tidigare känt kvalitetsproblem för extremt långa Qwen-frågeunderlag på CPU kvarstår. AI-utkast ska granskas före användning.

[Utförlig mötesguide](https://github.com/Pluggentipsar/avskrift/blob/v0.6.0/docs/ARBETSYTA-STEG-9.md)
