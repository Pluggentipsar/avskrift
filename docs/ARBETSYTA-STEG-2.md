# Arbetsyta 2 – original, versioner och återhämtning

2026-09-11. Fortsättning på [första byggsteget](ARBETSYTA-STEG-1.md) och [produktplanen](AVSKRIFT-NÄSTA.md).

## Det som går att använda

- **Original och versioner** i projektets överkant visar originalunderlaget och tidigare arbetskopior. Återställning sparar först den aktuella arbetskopian. Versionskopian innehåller hela projektet, inklusive anteckningar, utkast, inställningar och sparad granskning; textfältet visar ett utdrag av dess textinnehåll.
- Transkriptets första färdiga text behålls som original. För nya dokument fryses källtexten vid första bearbetningen. Äldre projekt behåller sitt befintliga underlag vid migrering. En tom, ännu inte transkriberad mötespost räknas inte som färdigt original.
- Fria källtexter sparas redan före bearbetning. Redigeringar sparas efter ungefär en halv sekund utan nya tangenttryckningar. Fel lämnar texten kvar och stoppar projektbyte/nytt arbete tills sparandet lyckas. Källtext kan också exporteras separat.
- Manuella maskningar, egna ersättningsord och granskningsval sparas med exakt den textversion de avser. Återöppning återställer granskningen utan en ny modellkörning. Om underlaget ändras finns den gamla granskningen kvar med en tydlig markering; export och AI-kopiering av den inaktuella granskningen blockeras tills den körts om.
- Genererade sammanfattningar får en referens till källtexten. Ändrad källtext markerar utkastet som inaktuellt. Äldre utkast utan sådan referens får inte en påhittad källkoppling.
- Ångra-historiken rensas vid projektbyte och nytt arbete. Originalet delas inte som ett redigerbart objekt med arbetskopian.
- Sparade diktat visas på startsidan och under Alla projekt, med sökning och direktöppning av rätt diktat. Sessionsdiktat finns fortsatt enbart i dikteringsvyn. Redigeringsfältet överlever navigering; ändringar sparas enligt diktatets befintliga val, på datorn eller bara under sessionen. Ett skrivfel lämnar redigeringen kvar.
- Word-import läser brödtext, tabellceller och nästlade tabeller i läsordning. Exporten återger text, inte originalets tabellayout. Sidhuvuden, sidfötter, bilder och textfält behöver fortsatt kontrolleras i originaldokumentet.

## Lagring och kompatibilitet

Projektbibliotekets JSON-format är nu version 2. Gamla projekt läses som tidigare; vid första sparandet behålls en exakt kopia som `jobs/versions/<id>/legacy.json`. Originalfält är beständiga. Okända JSON-fält bevaras; projekt med en högre formatversion skrivs inte över.

De senaste 30 versionskopiorna behålls, utöver migrationskopian. Vanligt autosparande arkiverar föregående arbetskopia högst en gång per minut. Bearbetning som stöds av kontrollpunkterna och återställning skapar dessutom en kopia. Versionskopior är alltså inte en kopia av varje tangenttryckning. De raderas tillsammans med projektet.

Projekt, diktat, fristående uppgifter och text-/Word-exporter skrivs till en tillfällig fil i samma katalog, synkas och ersätter sedan målfilen. Vid misslyckad skrivning behålls föregående målfil. Trasiga befintliga projekt skrivs inte över. Originalfiler som importeras ändras inte av bearbetningen.

Det äldre exportformatet `.avskrift` innehåller fortsatt transkript, talarnamn och ljudreferens. Använd projektbiblioteket för att behålla hela arbetsytan och dess versioner. Versionerna ligger lokalt på samma dator och är inte en separat säkerhetskopia.

Paketen använder samma appdatakatalog och modeller som tidigare. Kör en appversion åt gången. Äldre programversioner känner inte till det nya formatets granskningsfält; fortsätt därför redigera migrerade arbeten i det nya paketet.

## Verifiering

- `npm run check`: noll fel, tre tidigare befintliga varningar om två äldre dialogers fokusattribut och Node-typer.
- `node --test model-tools/save-queue.test.ts`: två passerade tester.
- Rust med Vulkan: 72 passerade tester, 11 explicita opt-in-tester utelämnade. Nya regressioner täcker atomisk ersättning/felsituation, migrering, oförändrat original, versionsgräns, återställning, trasig fil, validering av svenska UTF-8-offset, manuella ersättningsord och Word-tabeller.
- `model-tools/ui-smoke.cjs`: tidigare arbetsflöden, sparfel/återförsök, export och smal layout.
- `model-tools/ui-step2.cjs`: återöppna obearbetad källtext, bibliotek med endast sparade diktat, diktatändringar vid navigering/skrivfel, manuell maskning genom återöppning, återställning utan modell, inaktuell granskning/utkast, originalets oföränderlighet och isolerad Ångra-historik.

Webbläsartesterna använder headless Edge och syntetiska Tauri-kommandon. De läser inte användarprojekt eller spelar in ljud. Skärmbilder finns i `docs/ui-step2`. De nya native-paketen behöver fortfarande ett handprov av mikrofon, globala kortkommandon och native-fönsterstängning. Avbrott under pågående mötesinspelning och återställning av ännu osparade tangenttryckningar omfattas inte av de nya garantierna.

## Paket och fortsättning

- GPU: `dist/Avskrift-Vulkan-versioner/avskrift.exe`
- CPU: `dist/Avskrift-CPU-versioner/avskrift.exe`

Stäng den tidigare appen och starta filen i den nya paketmappen. Ingen ominstallation behövs för de portabla paketen; behåll hela mappen med DLL-filer och resurser.

Nästa produktsteg är källbelagda AI-utkast och återanvändbara mallar. Ett fullständigt bibliotek med flera källor per arbete, dokumentlayout sida vid sida och källankare genom alla typer av redigering återstår. Det här steget levererar lagring och återställning som den fortsättningen kan byggas på.
