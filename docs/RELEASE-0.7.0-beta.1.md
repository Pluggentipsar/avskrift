# Avskrift 0.7.0-beta.1 – från samtal till granskat utkast

Förhandsrelease med ett gemensamt mallflöde: ljudfil eller text → transkript → mallstyrt utkast → granskning → kopiering/export. Ingen integration med ärendehanteringssystem ingår och inget ärende registreras.

**Känd begränsning: den lokala modellens faktakvalitet är inte godkänd.** Verkliga prov med Qwen2.5 3B och 7B missade uttryckliga uppgifter i supportsamtalet. Kontrollera alla svar mot underlaget, även fält markerade som saknade. Använd det tydligt förberedda exemplet för en förutsägbar demonstration. Extern AI-kvalitet är inte provad. Version 0.6.0 ligger kvar som senaste stabila release.

## Nytt

- Inbyggd Supportärende-mall och egna namngivna mallar med fältinstruktioner, revisioner samt JSON-import/export.
- Separata redigerbara utkast med sparad mallversion och källkopia. Ny generering bevarar äldre utkast och manuella ändringar; transkriptet skrivs inte över.
- Granskning bredvid underlaget, markering av manuella ändringar och varning när underlaget ändrats.
- Samma mall för lokal bearbetning och förhandsvisad instruktion som kopieras manuellt till valfri godkänd AI-tjänst. Ingen automatisk överföring och ingen samtalstext i URL:er.
- Återklistrade AI-svar sparas som separata utkast. Förhandsvisning och export till TXT, Markdown och Word.
- Fiktivt supportdemo, syntetisk svensk ljudfil och exempelmall ingår.

## Hämta och starta

- **Windows-Vulkan.zip:** välj för dator med kompatibelt grafikkort, exempelvis RTX 5070 Ti.
- **Windows-CPU.zip:** välj för dator utan Vulkan-stöd eller när CPU-körning önskas.

Packa upp hela ZIP-filen till en ny mapp. Stäng den gamla AVskrift även i meddelandefältet och starta `avskrift.exe`. Ingen ominstallation behövs. Behåll medföljande DLL-filer och resurser i mappen. Whisper- och språkmodeller hämtas vid behov i appen; de ingår inte i ZIP-filerna.

Kortaste demo: **Sammanfatta text → Öppna fiktivt supportdemo med förberett utkast**. Utkastet är uttryckligen märkt som förberett och får inte presenteras som ett nygenererat modellresultat.

## Verifierat och återstående

- 104 ordinarie Rust-tester passerade på vardera CPU och Vulkan; 22 opt-in-tester ingick inte i den ordinarie sviten.
- Svelte/TypeScript: 0 fel, 3 tidigare varningar. Automatiska UI-tester för mallflödet och befintligt mötesflöde passerade med mockad Tauri-IPC.
- Verklig ljudavkodning och KB-Whisper-small provades på den syntetiska ljudfilen. De separata lokala modellkvalitetstesterna föll och är inte godkända.
- Full manuell provning av Windows-dialoger, urklipp, ljuduppspelning från hänvisningar och Word-exportens utseende återstår.

Se `docs/MALLFLODE-MVP.md` och `docs/demo-support/VERIFIERING.md` i paketet eller GitHub-repot för demonstration, gränser och testprotokoll. SHA256SUMS.txt innehåller kontrollsummor för de två ZIP-filerna.
