# Arbetsyta 5 – långa AI-underlag

2026-09-11. Tokenbaserad uppdelning för sammanfattningar, frågor, åtgärdsförslag och källutkast. Bygger vidare på arbetsyta 4.

## Vad som märks i programmet

- Långa transkript och dokument delas automatiskt efter hur mycket den valda modellen faktiskt kan läsa. Text, instruktioner, egna mallar och utrymme för svaret räknas tillsammans.
- Om alla delsammanfattningar inte ryms i ett slutsteg sammanställs de i flera omgångar. Samma princip gäller underlaget för frågor och åtgärdsförslag.
- Förloppet visar exempelvis **Bearbetar del 3 av 12**, **Sammanställer, omgång 2, del 1 av 3** eller **Söker svar, del 4 av 8**. Det är verkliga bearbetningssteg, inte en uppskattad tid till färdigt resultat.
- Källutkastets delar behåller ursprungliga käll-id:n, tidspositioner och text. Citat fortsätter kontrolleras mot just det utdrag modellen fick. Källutkasten sammanfogas som en lista med sina källkopplingar; de skrivs inte om genom en fri sammanfattning.
- En alltför omfattande mall eller fråga ger ett begripligt fel innan genereringen börjar. Den egna mötesmallen kan nu innehålla upp till 12 000 tecken i mallredigeraren; modellen kan ändå kräva en kortare mall beroende på tokeninnehållet.
- Ett ofullständigt eller tomt modellsvar blir ett fel, inte ett färdigt utkast. Tidigare sammanfattning, källutkast och obesvarad fråga behålls, och det går att försöka igen.
- Ytterligare AI-körningar och byte till ett annat projekt blockeras under den pågående körningen. Frågor och åtgärdsförslag visar också arbetsförloppet. Ett uttryckligt svar om att inga åtgärder hittades läggs inte till som en åtgärd.

## Hur budgeten fungerar

Den gemensamma motorn räknar med tokenizern som finns inbyggd i den laddade GGUF-filen – samma tokenizer och inställning som används vid inferens. Varje fullständig prompt kontrolleras, inklusive chattformat och JSON-kodning. Det effektiva arbetsfönstret är det minsta av modellens tränade fönster och appens tak på **8 192 token**. Taket höjs inte automatiskt, vilket håller behovet av KV-minne begränsat.

Varje steg reserverar sitt eget svarutrymme: 1 024 token för en slutlig sammanfattning, 512 för mellananteckningar eller ett slutligt frågesvar, 384 för mellananteckningar till frågor och 1 400 för ett källutkast. Kontextkontrollen finns även kvar omedelbart före inferens.

`text_budget.rs` delar text vid giltiga UTF-8-gränser och föredrar rad- eller ordgränser nära delens slut. Delarna kan fogas ihop till exakt samma originaltext, inklusive svenska tecken och radbrytningar. Långa enskilda rader hanteras utan den tidigare byteuppdelningens risk för ersättningstecken. Varje vald brytpunkt kontrolleras på nytt eftersom tokenantal inte alltid förändras jämnt med textlängden.

För frågor samlas först frågerelevanta sakuppgifter. Slutfrågan besvaras från det samlade underlaget, som vid behov komprimeras i flera omgångar. Frågor och källutkast använder dessutom ett mål på cirka **2 400 token källinnehåll per del**, inom den fullständiga promptens hårda gräns. Att tekniskt fylla hela arbetsfönstret gav sämre faktabevarande i det syntetiska provet med den mindre modellen.

Sammanställningen måste minska mängden token mellan omgångarna och stoppas annars med ett fel. Högst tolv extra sammanställningsomgångar tillåts. Detta skyddar mot en modell som upprepar eller utökar underlaget så att processen aldrig blir klar.

## Verifiering

- **77 Rust-tester passerar**, 13 opt-in-tester utelämnas i den vanliga sviten.
- Tester med en avsiktligt liten kontext kontrollerar varje anrop och tvingar fram flera sammanställningsomgångar för både sammanfattning och frågor. För stora instruktioner, avbrutna svar och utebliven komprimering ger fel utan delresultat.
- Uppdelning provas med långa rader, svenska tecken, emoji, CJK-tecken, radbrytningar och tokenantal som inte är monotona vid brytpunkterna.
- Ett lokalt Qwen 2.5 3B-prov använder ett syntetiskt underlag på **10 346 token inklusive sammanfattningsprompt**, med utvalda fakta i början och slutet. Testet kontrollerar bland annat namn, dagar och färg i sammanfattning och frågesvar, antal i sammanfattningen samt att frågesvaret inte ändrar en planerad beställning till leverans eller genomförd beställning.
- Samma modellprov kontrollerar långa egna instruktioner, JSON-escaping, käll-id:n och tidspositioner med modellens verkliga tokenizer.
- **GPU:** det långa modellprovet passerar de utvalda faktakontrollerna.
- **CPU:** det separata provet för källutkast/diktatbearbetning passerar. Den långa sammanfattningen klarar faktakontrollerna och hela frågekedjan blir klar inom budgeten, men **frågesvarets kvalitetskontroll fallerar**: slutsteget tappar Åsa och färgen blå samt ger en felaktig formulering. Det strikta opt-in-testet är kvar och underkänns på denna CPU-körning; det räknas inte in i de 77 vanliga regressionstesterna. Loggen bekräftar 0 av 37 lager på GPU. Utfallet visar en faktisk kvalitetsbegränsning i den här kedjan med Qwen 2.5 3B, inte ett godkänt frågesvar.
- Alla fem UI-sviter och fem hjälptester passerar. Den nya UI-sviten provar synligt förlopp, fel under senare steg, bevarade utkast/frågor, nytt försök och skydd mot byte av projekt under pågående AI-körning.
- `npm run check` visar noll fel och tre tidigare varningar: två äldre dialogers fokusdeklaration och Node-typdefinitioner.

UI-testerna använder simulerad IPC. Modellproven använder en redan befintlig lokal GGUF och enbart syntetisk text. Inga riktiga möten läses och inga modeller hämtas i testerna.

## Praktiska gränser

Detta gör stora underlag hanterbara inom modellens arbetsfönster. Det garanterar inte att modellen fångar alla relevanta uppgifter eller formulerar dem korrekt. Små modeller kan fortfarande utelämna detaljer, skriva klumpigt eller göra felaktiga tolkningar. Fler sammanställningssteg kan förlora nyanser; granska resultatet mot originalet. De syntetiska kontrollerna är ingen bred kvalitetsutvärdering av verkliga möten.

En lång mall tar plats även när källtexten delas upp. Om mallen i sig nästan fyller arbetsfönstret måste den kortas. Ett önskat svar som överstiger stegets svarutrymme ger också ett fel. Ett nytt försök startar om bearbetningen; delresultat cachas inte för återupptagning.

Källutkastets befintliga gräns på 500 000 byte källtext och diktatbearbetningens gräns på 6 000 tecken är kvar. Automatisk minnesbudget, frigöring av inaktiva modeller och CPU-reservväg vid minnesbrist ingår inte i denna leverans.

## Paket

- GPU: `dist/Avskrift-Vulkan-arbetsyta5/avskrift.exe`
- CPU: `dist/Avskrift-CPU-arbetsyta5/avskrift.exe`

Stäng tidigare AVskrift och starta programfilen i det nya paketet. Behåll hela paketmappen. Ingen ominstallation behövs. Befintliga projekt och hämtade modeller används fortsatt. Sidomenyn visar **arbetsyta 5**.
