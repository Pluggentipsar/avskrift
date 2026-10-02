# Avskrift 0.7.0-beta.2 – Pianissimo som valbar talmotor

Förhandsrelease som lägger till Pianissimo svenska som experimentellt, valbart CPU-alternativ för ljudfiler, möten och diktering. KB-Whisper är fortfarande standard och tidigare sparade modellval behålls. Allt från 0.7.0-beta.1 (mallflöde, utkast och granskning) ingår oförändrat, inklusive dess kända begränsning: den lokala modellens faktakvalitet i utkast är inte godkänd. Version 0.6.0 ligger kvar som senaste stabila release.

## Nytt

- **Pianissimo svenska — CPU (experimentell)** under **Modeller på datorn**. Cirka 923 MB hämtas och kontrolleras med SHA-256; modellen förbereds därefter lokalt (kan ta flera minuter första gången). Python behövs inte.
- Separata modellval för möten/ljudfiler och diktering.
- Minneskontroll före modellladdning som räknar med både ledigt RAM och Windows reservationsutrymme (inklusive växlingsfil). Laddade modeller frigörs vid inaktivitet eller minnestryck.
- Långa ljud bearbetas i avsnitt på 28 sekunder med överlapp. Endast svenska; ungefärliga segmenttider och ingen ordmarkering för Pianissimo.

## Hämta och starta

- **Windows-Vulkan.zip:** GPU-version för dator med kompatibelt grafikkort, exempelvis RTX 5070 Ti. Whisper och de lokala språkmodellerna körs på grafikkortet.
- **Windows-CPU.zip:** för dator utan Vulkan-stöd eller när CPU-körning önskas.

Pianissimo körs på processorn i båda paketen; Vulkan-versionen påskyndar KB-Whisper och språkmodellerna.

Packa upp hela ZIP-filen till en ny mapp. Stäng den gamla Avskrift även i meddelandefältet och starta `avskrift.exe`. Ingen ominstallation behövs. Behåll medföljande DLL-filer och resurser i mappen. Whisper-, Pianissimo- och språkmodeller hämtas vid behov i appen; de ingår inte i ZIP-filerna.

## Verifierat och återstående

- 107 ordinarie Rust-tester passerade på vardera CPU och Vulkan (release-profil); 24 opt-in-tester ingick inte i den ordinarie sviten. Pianissimo-motorns 8 enhetstester passerade.
- Svelte/TypeScript: 0 fel, 3 tidigare varningar.
- Båda paketen byggda med optimerad whisper.cpp (Ninja, /O2). Varje exe- och DLL-fil i paketen kontrollerades mot sina beroenden; inga saknades. CPU-paketet har inget Vulkan-beroende.
- Paketen startades inte i detta bygge. Pianissimos kvalitet på naturliga inspelningar, flera talare och brus är inte bedömd; se mätningar och begränsningar i `docs/PIANISSIMO.md`.

Se `docs/PIANISSIMO.md` i paketet eller GitHub-repot för funktion, begränsningar och mätningar. SHA256SUMS.txt innehåller kontrollsummor för de två ZIP-filerna.
