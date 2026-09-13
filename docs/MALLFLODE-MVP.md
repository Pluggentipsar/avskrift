# Skapa från mall – MVP och supportdemo

Förhandsrelease **0.7.0-beta.1** finns som Windows-paket för Vulkan och CPU på [GitHub](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.7.0-beta.1). Packa upp hela ZIP-filen. Stäng den gamla appen även i meddelandefältet och starta `avskrift.exe` i den nya programmappen. Behåll hela mappen med bibliotek och resurser. Sidfoten visar **0.7.0-beta.1 · mallflöde**. Vulkan rekommenderas för separat grafikkort; CPU-paketet fungerar utan Vulkan.

## Flödet

Öppna en ljudfil i **Möten och transkribering**, transkribera och rätta texten. Öppna fliken **Skapa från mall**. Samma arbete behåller ljud, transkript och alla mallutkast. Funktionen finns också vid **Sammanfatta text** för inklistrad eller importerad källtext.

1. Välj **Supportärende** eller en egen dokumentmall.
2. Välj originaltext med eventuella rättningar eller aktuell avidentifierad text. Maskerad text kan bara användas när granskningen hör till det aktuella underlaget. Namngivna talaretiketter återinförs inte i maskerad text.
3. **Bearbeta lokalt** fyller ett fält i taget med den valda lokala textmodellen. Hela underlaget används för varje fält. **Fyll i själv** öppnar samma mall utan modellkörning.
4. **Kopiera för annan AI** visar instruktion, mall och exakt underlag före kopiering. Klistra själv in paketet i en för verksamheten godkänd tjänst. Appen öppnar ingen extern tjänst i detta flöde. Den äldre öppna-funktionen använder nu också tomma tjänsteadresser utan text i URL:en; dess Copilot-länk är ingen anvisning om vilken tjänst kommunen har godkänt.
5. Ett återklistrat AI-svar sparas som ett separat, ogranskat textutkast. Automatisk fördelning av ett externt svar till mallfält ingår inte. Ett väntande kopieringspaket bevaras tills svaret sparats eller överlämningen avslutas.
6. Granska och redigera utkastet bredvid dess sparade underlag. Redigering nollställer fältets granskningsmarkering och märks som en manuell ändring. Ny generering lägger till ett nytt utkast; den ersätter inte tidigare redigeringar.
7. **Förhandsvisa och kopiera/exportera** visar dokumenttexten före kopiering eller export till Markdown, text eller Word. Texten anger granskningsstatus. Inget ärende registreras.

En källhänvisning visas endast om dess text faktiskt finns i den sparade källan. I den lokala MVP-vägen skapas en automatisk hänvisning bara när svaret ordagrant finns i ett källsegment; sammanfattande formuleringar saknar normalt sådan hänvisning. Hela underlaget finns ändå bredvid. En giltig texthänvisning bevisar inte att en slutsats är riktig.

## Egna mallar

Välj **Ny mall**, **Duplicera mall** eller **Redigera mall**. Standardmallen kopieras automatiskt när den redigeras. Ange namn, syfte och 1–12 ordnade fält. Varje fält har rubrik, instruktion/fråga och text för saknad uppgift. Fält-id skapas av appen och följer med vid omordning.

Egna mallar sparas i `document-templates-v1.json` intill appens projektkatalog. Samma bank används för lokal bearbetning och AI-kopiering. Äldre mötesmallar, diktatmallar och gamla kopieringsprompter ändras inte automatiskt av detta MVP.

Import/export använder UTF-8 JSON. Importerade mallar öppnas först i redigeraren och måste sparas för att användas. Import skapar en ny mallidentitet; den ersätter inte en befintlig mall med samma id. Okänd formatversion, okända egenskaper, dubbla fält-id, ogiltiga längder och mallfiler över 64 KiB avvisas. Varje sparning höjer mallens revision. Utkast lagrar hela mallkopian, revisionen, källkopian, bearbetningsvägen och tidpunkten.

Exempel: [supportarende.mall.json](demo-support/supportarende.mall.json). `formatVersion` avser filformatet; `revision` avser mallens innehållsversion.

## Kortaste demonstration: förberett exempel

1. Öppna **Sammanfatta text**.
2. Klicka **Öppna fiktivt supportdemo med förberett utkast**.
3. Visa att omstarten inte hjälpte, att platsen är plan två och att kontaktuppgifter saknas.
4. Komplettera utrustningsfältet med exempelvis `DEMO-17`. Visa markeringen för manuell ändring och granskningsrutan.
5. Förhandsvisa och kopiera dokumentunderlaget.

Exemplet är författat i förväg och märkt **FIKTIVT DEMO**. Reservutkastet visas som **Förberett demo – inte ett modellresultat**; det ska aldrig presenteras som ett nygenererat AI-resultat. Ingen ljudfil hör till den förberedda källkopian, så den innehåller inga påhittade ljudhänvisningar. Knapparna skapar ett nytt sparat demoarbete varje gång.

## Demonstration med verklig ljudimport

1. Importera [fiktivt-supportsamtal.wav](demo-support/fiktivt-supportsamtal.wav) som vanlig ljudfil. Filen är syntetiskt svensk tal med Windows-rösten Microsoft Bengt; inga verkliga samtal eller personuppgifter används.
2. Transkribera på svenska med KB-Whisper-small. Granska att `plan två`, `Det hjälpte inte` och supportens undersökning av anslutningen bevarats.
3. Öppna **Skapa från mall** och använd Supportärende. Välj lokal modell eller förhandsvisa kopieringspaketet.
4. Kontrollera även osäkerheten: det är inte känt om andra användare berörs. Inget skrivar-id, ingen kontaktuppgift eller återkopplingstid får uppfinnas.

Röstnamnen ”Användare” och ”Support” läses också upp i den syntetiska filen. Den provar import och transkribering, inte realistisk talarseparering mellan två olika mänskliga röster.

## Avgränsningar

- Första versionen accepterar högst 60 000 byte underlag. Lokal bearbetning kontrollerar dessutom varje komplett fältprompt mot modellens verkliga tokengräns och reserverar 800 svarstoken. Överskrids gränsen får användaren ett fel före genereringen. Ingen text kapas eller sammanfattas bort automatiskt.
- Saknade och motstridiga uppgifter identifieras av modellen; detta är inte en deterministisk faktakontroll. Granska också fält som markerats som saknade. En prompt kan inte garantera att modellen aldrig följer instruktioner i underlaget eller hittar på innehåll.
- Ett ändrat underlag gör äldre utkast inaktuella. Deras källkopior och redigeringar bevaras och kan läsas och exporteras, men markeras för ny granskning.
- Ingen integration med ärendehanteringssystem, automatisk registrering, PDF-mallimport eller avancerad dokumentlayout ingår.
- Mallredigerarens pågående text sparas med arbetet när ett underlag finns. Spara själva mallen med **Spara mall** för att den ska kunna användas i andra arbeten.

## Verifiering

Se [testprotokollet](demo-support/VERIFIERING.md) för skillnaden mellan programtester, syntetisk ljudprovning och bedömning av modellkvalitet.
