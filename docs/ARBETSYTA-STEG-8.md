# Arbetsyta 8 – snabbare projektbibliotek

2026-09-11. Nästa steg efter arbetsyta 7: sökning och projektlistor som fungerar bättre när historiken växer.

## I programmet

- Projektbiblioteket använder ett lokalt sökindex. Vid första användningen läses befintliga projekt in; därefter läses bara nya eller ändrade projektfiler. Ingen manuell flytt eller konvertering behövs.
- Sökningen behåller delsträngssökning utan skillnad på stora och små bokstäver. Svenska tecken, korta frågor, citattecken och tecken som `%` och `_` fungerar fortsatt som vanlig text. Titel, mapp, transkript, källtext, sammanfattning, anteckningar, deltagare, ansvariga och åtgärdstext ingår som tidigare.
- Listor och Åtaganden kan också använda de sparade metadatauppgifterna utan att läsa alla transkript. Ljudfilernas aktuella storlek kontrolleras separat, så borttaget ljud inte fortsätter räknas.
- Sökstatus visas medan sökningen pågår. Ett äldre svar eller fel får inte ersätta en nyare sökning. Vid fel finns **Försök söka igen**.
- **Uppdatera biblioteket** återskapar sökuppgifterna från projektfilerna. Den aktuella sökfrågan behålls, liksom projekt och versioner. Normala sparningar och ändringar hanteras automatiskt.
- Projektkommandon som kan behöva vänta på lagringen körs utanför gränssnittets tråd. Ändring av projektmetadata och åtgärder läser och skriver under samma lås, så samtidiga tillägg inte tappas bort.

## Lagring och återhämtning

Projektens JSON-filer är fortfarande originalen. Den nya filen `.library-v1.sqlite` i programmets projektkatalog är en återskapningsbar kopia av sökbar text och metadata. Den innehåller lokala sökuppgifter och ska behandlas som projektdata. Inget skickas till en server.

Indexet använder inbäddad SQLite genom [rusqlite](https://docs.rs/rusqlite/0.37.0/rusqlite/). [SQLite FTS5:s trigramindex](https://www.sqlite.org/fts5.html#the_trigram_tokenizer) hittar delsträngar från tre tecken; kortare frågor söker i cachade textfält. Kandidater kontrolleras mot de enskilda fälten så att en träff inte skapas av exempelvis slutet på titeln och början på mappnamnet.

Sparningar markerar projektet för uppdatering. Borttagning tar bort projektets indexposter. Återställning av versioner och flytt av mappar går genom samma lagring. Vid varje biblioteksfråga jämförs filstorlek, ändringstid och tillgänglig skapandetid. Det fångar också ändringar utanför programmet och ett avbrott mellan projektsparning och indexuppdatering. Om innehållet ändras samtidigt som dessa filuppgifter avsiktligt bevaras behövs **Uppdatera biblioteket**.

Ett skadat eller inkompatibelt index återskapas automatiskt. Om indexet inte går att använda finns den tidigare läsningen direkt från projektfilerna kvar som reservväg. Ett indexfel gör inte en lyckad projektsparning till ett sparfel. Trasiga projektfiler behålls på disk och hoppas över; deras gamla indexträffar visas inte. Indexet har en SQLite-sidcache på cirka 8 MiB per anslutning; bearbetning av ett enskilt stort projekt kan kräva mer RAM.

## Verifiering och mätning

- **96 vanliga Rust-tester** passerar på CPU och Vulkan. 19 prov är opt-in; ett av dem är den nya biblioteksmätningen.
- Nya tester jämför sökresultaten med den gamla sökningen, inklusive svenska tecken, korta frågor, specialtecken och fältgränser. De provar ändringar av innehåll, mappar, åtgärder, ljud och versioner, extern ändring/borttagning, skadat index, obrukbar cache, trasiga projekt och samtidiga åtgärdstillägg och mappflyttar för fristående åtaganden.
- Upprepade sökningar, listning och åtgärdsläsning läser **0 projekt-JSON-filer** när inget ändrats. Återöppning av databasen kräver inte heller att alla projekt läses in igen.
- UI-proven täcker sökrace, vänteläge, fel/nytt försök, uppdatering med bibehållen fråga, bevarade original och smal vy. Regressionerna för arbetsyta 2, 3 och 7 passerar. `npm run check` ger noll fel och tre tidigare varningar.

På utvecklingsdatorn, med **1 000 syntetiska projekt** och cirka 13 250 tecken extra källtext per projekt:

| Mätning | Resultat |
|---|---:|
| Första indexbygget | 693 ms |
| Ny sökning, medel av 10 körningar | 2,65 ms |
| Tidigare sökning, medel av 10 körningar | 67,52 ms |
| Indexfil på disk | cirka 43,5 MiB |

Det motsvarar cirka **25 gånger snabbare sökning i detta prov**. Siffrorna gäller backend med varm filcache, inte tiden för att rita hela gränssnittet. Maskin, disk, projektstorlek och antal träffar påverkar resultatet. Ingen mätning på en faktisk kommundator gjordes. Mycket stora projektlistor renderar fortfarande alla rader; indexet löser läsning och sökning, inte den delen av UI-renderingen.

Tal- och textmodellernas kvalitet ändras inte av detta steg. Den tidigare begränsningen för långa CPU-frågesvar med Qwen 2.5 3B kvarstår, liksom arbetsyta 7:s avbrottsgränser.

## Starta

- GPU: `dist/Avskrift-Vulkan-arbetsyta8/avskrift.exe`
- CPU: `dist/Avskrift-CPU-arbetsyta8/avskrift.exe`

Stäng tidigare AVskrift och starta den nya programfilen. Behåll hela paketmappen. Ingen ominstallation behövs. Sidomenyn visar **arbetsyta 8**.
