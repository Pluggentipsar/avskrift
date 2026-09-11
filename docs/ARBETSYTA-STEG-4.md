# Arbetsyta 4 – läsa, hitta och arbeta vidare

2026-09-11. Bygger vidare på arbetsyta 3. Denna leverans förbättrar transkriptets arbetsyta och samlar modellvalen. Tokenbudget, modellernas minneshantering, strömmande ljudavkodning, historikindex och diariseringens backend är fortsatt separata steg.

## I programmet

- **Modeller på datorn** finns i sidomenyn och kan öppnas från respektive arbetsflöde. Dialogen lämnar det pågående arbetet kvar bakom sig. Här väljer och hämtar du talmodell för möten/ljudfiler, talmodell för diktering och textmodell för sammanfattning, frågor, åtgärdsförslag och diktatbearbetning.
- Diktering har kvar sitt eget talmodellval; hämtade talmodeller delas. Ett misslyckat diktatmodellval visar ett fel och återställer visat val. Modellval låses under pågående inspelning/bearbetning. Hämtning kräver internet och visar förlopp eller ett fel som går att försöka igen efter.
- Textmodellvalet kommer ihåg senaste inställningen. Ett projekt med ett tidigare sparat textmodellval återställer det vid öppning. Avidentifieringens egna identifierings- och granskningsmodeller påverkas inte av detta val.
- **Transkript** öppnas med läsytan i fokus. Rättning av återkommande fel, talaruppdelning och omtranskribering nås genom **Visa verktyg för transkriptet**. Dolda verktyg tar inte emot tangentbordsfokus.
- **Sök i transkript** söker i hela texten, inklusive de avsnitt som inte visas. Ctrl+F öppnar sökfältet. Pilknapparna eller Enter/Shift+Enter går till nästa/föregående matchande avsnitt. Räknaren räknar avsnitt, inte varje enskild förekomst av sökordet. Aktuell träff har en markerad kant och bakgrund.
- **Textstorlek** har fyra lägen och sparas på datorn. **Följ uppspelningen** håller det aktuella avsnittet synligt. Rullning med mus/touch, sökning eller redigering släpper följningen; kryssa i den igen för att återgå.
- När ett textavsnitt har fokus byter upp/ned-pil avsnitt. Ctrl+Home/End går till första/sista avsnittet. Enter eller F2 börjar rätta. Ctrl+Enter sparar rättningen, Escape avbryter. Tidsknappar spelar från avsnittets början; klick på ord med tidsstämplar spelar från ordet.
- Källutkast och fria utkast har tydligare åtskillnad: inställningar för ett **fritt utkast utan källhänvisningar** är infällda i sidopanelen.

## Prestanda och struktur

`TranscriptView.svelte` äger nu läsning, sökning, tangentbordsnavigation och uppföljning av uppspelning. `TranscriptBlock.svelte` visar innehåll nära läsytan och behåller uppmätta höjder för övriga block. Pågående redigering och fokuserade block hålls kvar vid rullning. `ModelSettings.svelte` ersätter upprepade modellväljare och hämtningsflöden i huvudvyn.

Transkriptet delas vid högst 16 avsnitt eller ungefär 350 ord per block. En enskild replik delas inte för visningen och kan därför överskrida ordgränsen. Ingen transkripttext tas bort; kopiering och export använder fortfarande hela underlaget. Vid kopiering av hela transkriptet används arbetsytans **Kopiera** eller **Exportera** – vanlig markering i läsytan omfattar det renderade innehållet.

Uppspelning hittar aktiv replik med en indexerad tidsuppslagning som också hanterar överlappande talare och luckor. Osorterade äldre transkript får en kompatibel linjär reservväg. Scrollning sker när aktivt avsnitt ändras, med en kort stabilisering när blockens höjder mäts. Ordmarkeringar uppdateras endast i monterade block.

## Verifiering

- `npm run check`: noll fel. Tre befintliga varningar kvarstår: två äldre dialogers fokusdeklaration och Node-typdefinitioner.
- Fem hjälptester: sparningskö, tidsuppslagning inklusive överlapp/luckor, blockgränser och sökning med svenska tecken.
- Alla fyra UI-sviter: `ui-smoke.cjs`, `ui-step2.cjs`, `ui-step3.cjs` och `ui-step4.cjs`.
- Nya UI-testet använder 2 000 avsnitt med **50 000 tidsstämplade ord**. Vid första visningen monterades **350 ordknappar**, i stället för tidigare 50 000. Detta mäter mängden renderat innehåll, inte transkriberingshastighet eller en generell hastighetsfaktor.
- Testar sökning långt utanför visningen, nästa/föregående och omslag, tangentbordsnavigation, redigering som behålls vid rullning, spara/avbryt/ångra, byte av talare, borttagning, uppspelning från enskilda ord, uppspelningsföljning, större text och smalt fönster. Modellflödet provas med separata talmodellval, delad textmodell, misslyckad hämtning med nytt försök och misslyckat inställningssparande.
- UI-testerna körs i headless Edge med simulerad IPC och ljudklocka. De läser inga riktiga projekt, spelar inte in ljud och hämtar inga modeller. Den verkliga WebView2-uppspelningen behöver också provas i det byggda programmet. Tidigare native modelltester har inte körts om för denna UI-leverans.

Skärmbilder och en syntetisk mätning finns i `docs/ui-step4/`.

## Starta versionen

- GPU: `dist/Avskrift-Vulkan-arbetsyta4/avskrift.exe`
- CPU: `dist/Avskrift-CPU-arbetsyta4/avskrift.exe`

Stäng den tidigare AVskrift-versionen och starta programfilen i det nya paketet. Behåll hela paketmappen. Ingen ominstallation behövs. Sidomenyn visar **arbetsyta 4**. Befintliga projekt och modeller används fortsatt.
