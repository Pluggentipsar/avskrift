# Ny arbetsyta – första byggsteget

2026-09-11. Implementerar det första steget i [produktplanen](AVSKRIFT-NÄSTA.md).

## Det som är genomfört

- Fast huvudnavigation och tre likvärdiga ingångar: Möten, Diktering och Avidentifiering.
  Alla projekt, Åtaganden och fristående sammanfattning går alltid att nå.
- Lokala flikar för transkript, anteckningar, avidentifiering, sammanfattning och frågor
  ligger i det aktuella arbetet. Ordmärket tar användaren hem utan att radera arbetet.
- Större hjälptext, mörkare sekundärtext, indigofärgad navigation, tydliga fokusmarkeringar,
  anpassning till smalare fönster och respekt för minskad rörelse.
- Diktering visar senaste diktatet öppet. Tidigare diktat och inställningar kan fällas ihop.
  Skillnaden mellan sessionsinnehåll och sparade diktat finns kvar.
- Redigerade transkript, talarnamn, sammanfattningar och mötesanteckningar sparas med
  fördröjning. Status skiljer på väntande ändringar, pågående sparande, lyckat sparande och fel.
  Sparfel ligger kvar med nytt försök och möjlighet till export.
- Oföränderliga projektsnapshots skrivs i ordning. Byte till ett annat sparat projekt och
  nytt arbete inväntar sparande; misslyckad skrivning stoppar återställningen.
  Fönsterstängning inväntar dessa projektsparningar.
- Gemensam exportdialog för transkript, avidentifierad text, sammanfattning och anteckningar.
  Endast relevanta format erbjuds. Tidsstämplar och bifogat transkript väljs i dialogen.
  Kopiering och filskrivning använder den visade texten, inte ett senare motortillstånd.
  Word-förhandsvisningen visar textinnehållet, inte en renderad sidlayout.
- Projektets typ behålls vid arbete med dess sammanfattning; ett möte blir inte en annan
  projekttyp bara för att användaren bearbetar resultatet.
- Formuleringen som utlovade att avidentifierad text var trygg att skicka till extern AI
  ersätts med besked om valda maskeringar och kontroll före delning.

## Komponenter och kontroll

`AppNavigation.svelte` och `ExportDialog.svelte` separerar appskal respektive export från
huvudvyn. `save-queue.ts` serialiserar skrivningar. Rust-kommandot `preview_transcript`
använder befintliga formaterare för text, Word-innehåll och undertexter.

Verifiering:

- `npm run check`: noll fel. Tre sedan tidigare befintliga varningar: två äldre dialogers
  fokusattribut och saknade Node-typdefinitioner.
- `node --test model-tools/save-queue.test.ts`: två passerade tester för ordnade skrivningar
  och återhämtning efter lagringsfel.
- `model-tools/ui-smoke.cjs`: headless Edge med syntetiska IPC-fixtures. Kontrollerar
  navigation, autosparande, diskfel/nytt försök, skyddad återställning, exportens innehåll,
  Escape, diktering, avidentifiering och smal startsida. Skärmbilder finns i `docs/ui-step1`.
  Kör mot `npm run dev` och sätt `AVSKRIFT_PLAYWRIGHT` till installerad Playwright-modul
  om den inte kan lösas via Node. Inga riktiga inspelningar eller användarprojekt läses.
- Rust-regressioner med Vulkan: 68 passerade, 11 explicita opt-in-tester utelämnade.

Webbläsartesterna simulerar native-kommandon. De ersätter inte ett handprov av mikrofon,
globala kortkommandon, filväljare och fönsterstängning i den paketerade Windows-appen.

## Paket

Separata portabla paket skapas i `dist/Avskrift-Vulkan-arbetsyta` och
`dist/Avskrift-CPU-arbetsyta`. Starta `avskrift.exe` ur hela paketmappen. Ingen ominstallation
behövs för dessa paket. Tidigare paket ligger kvar; modeller och appdata använder samma
datakatalog som tidigare. Starta endast en version i taget eftersom kortkommandon och
lagring delas.

## Nästa steg och kvarvarande gränser

Uppdatering 2026-09-11: flera av gränserna nedan är nu åtgärdade i [arbetsyta 2](ARBETSYTA-STEG-2.md). Avsnittet beskriver läget vid leveransen av steg 1.

Detta är första byggsteget, inte hela produktplanen. Gemensamt bibliotek för alla källor,
versionshantering, källbelagda AI-resultat och förbättrad Word-import återstår.

Historiken återskapar fortfarande avidentifiering från underlaget. Manuella maskeringar
lagras ännu inte som beständiga granskningsbeslut; vyn säger därför uttryckligen att
resultatet behöver exporteras. Projektfilerna har samma format som tidigare och har ännu
inte migrerats till atomiska skrivningar eller en versionsdatabas.

Fria källtexter före första bearbetning och osparade ändringar i ett diktats redigeringsfält
har ännu inte samma autosparflöde som ett bearbetat projekt. Dikteringens knappar för att
behålla ändringar och spara diktat styr fortsatt lagringen. Ett fullständigt test av alla
äldre vyer vid kraftig förstoring återstår.
