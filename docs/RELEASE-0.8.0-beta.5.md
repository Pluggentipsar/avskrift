# Avskrift 0.8.0-beta.5 – sax i Textklipp och stabilare talarseparering

Förhandsrelease. Kan installeras med **Sök efter uppdatering** i appen från 0.8.0-beta.3 eller
senare. Allt från tidigare förhandsversioner ingår. Version 0.6.0 ligger kvar som senaste stabila
release.

## Nytt

- **Sax och Markera i Textklipp.** Välj Sax (C) och klicka på vågformen eller tidslinjen för att
  dela, eller Dela vid markören (S) medan du lyssnar. Välj sedan Markera (V), klicka på biten mellan
  två delningar och tryck Delete. Du kan också dra över vågformen för att markera ett avsnitt.
  Klippen görs i tid, oavsett vilka ord som ligger där.
- **Långa tystnader markeras.** Tysta partier hittas i ljudet, även där ord ligger utspridda över
  tystnaden, och visas med ⏸ i texten och på tidslinjen. Klicka för att ta bort ett, eller ta bort
  alla på en gång. En kvarts sekund behålls i varje kant så att skarvarna inte blir hackiga.
- **Omtagningar markeras i texten.** Den tidigare tagningen stryks under och får ett ↺ som tar bort
  den med ett klick. Utöver hela meningar som sägs igen hittas nu också omstarter mitt i en mening,
  men inte medvetna upprepningar.
- **Talarsepareringen kraschar inte längre** på långa inspelningar. Felet låg i biblioteket som
  gör separeringen och kunde stänga appen, särskilt med en talare. Separeringen sköts nu delvis av
  Avskrifts egen kod, med samma resultat som tidigare, och går att avbryta.

## Hämta och installera

**Har du 0.8.0-beta.3 eller senare installerad:** välj **Sök efter uppdatering** i menyns
statusruta och **Ladda ner och installera**.

Annars:

- **Avskrift_0.8.0-beta.5_x64-setup-vulkan.exe:** installationsprogram för dator med
  kompatibelt grafikkort.
- **Avskrift_0.8.0-beta.5_x64-setup-cpu.exe:** installationsprogram för dator utan Vulkan-stöd.
- **Windows-Vulkan.zip / Windows-CPU.zip:** portabla mappar utan installation (uppdateras inte i
  appen).

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar för hand. Dina arbeten
ligger kvar i appens datamapp. Installationsprogrammen är inte kodsignerade, så Windows
SmartScreen kan varna. Välj *Mer information* och *Kör ändå*.

## Verifierat och återstående

- Rust-tester på vardera CPU och Vulkan, bland dem nya för tystnader, delningar och
  talarsepareringen. Talarsepareringen kördes sex gånger på en 10 minuters inspelning, två av dem
  med en talare (det som kraschade förut), utan krasch och med samma resultat som tidigare.
- Omstartsdetektorn prövades på en egen 32 minuters inspelning: två förslag, båda riktiga.
- UI-tester (mockade data) för Textklipp med sax, markering, tystnader och omtagningar, samt
  möten, sammanfattningar och grundflödet. Svelte/TypeScript: 0 fel.
- Båda installationsprogrammen är signerade och signaturerna kontrollerade mot nyckeln i appen.
- Inte provat: klippning med sax på en riktig lång inspelning i appen.

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
