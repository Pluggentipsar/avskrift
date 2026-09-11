# Arbetsyta 6 – gemensam minneshantering

2026-09-11. Bygger vidare på arbetsyta 5. Fokus: undvika dubbla modellkopior, anpassa GPU-användningen och frigöra inaktiva modeller.

## I programmet

- AI-granskning, källutkast, sammanfattning, frågor och diktatbearbetning delar nu en laddad textmodell. Samma modellfil behöver inte ligga i minnet en gång per funktion. Vid modellbyte frigörs den gamla kopian först.
- Talmodellens vikter och beräkningsbuffertar återanvänds mellan diktat och transkriberingar. Det finns högst en laddad talmodell och en laddad textmodell i dessa cacher.
- Textmodellen anpassar antal GPU-lager efter ledigt grafikminne. Hela arbetsfönstret på upp till 8 192 token behålls. På en rymlig GPU kan hela modellen avlastas; på en mindre kan GPU och CPU dela arbetet.
- Inaktiva text-, tal- och NER-modeller frigörs efter ungefär två minuter. Nästa användning laddar dem igen. Vid minnestryck kan modeller som varit oanvända i minst tio sekunder frigöras tidigare. Kontroll sker var femtonde sekund.
- Innan en annan stor modell laddas frigörs den inaktiva motparten vid behov. Pågående inferens skyddas av ett gemensamt lås; den blir aldrig avlastad mitt i en beräkning.
- Återhämtningsbara fel från GPU-modellens laddning, kontext eller beräkning kan ge ett nytt försök på CPU. Samma text/ljud används, GPU-allokeringen släpps först och bara ett färdigt resultat lämnas tillbaka. Ett misslyckat CPU-försök ger ett fel, ingen oändlig återförsöksslinga.
- Under **Modeller på datorn → Minne och bearbetning** visas ledigt RAM, rapporterat GPU-minne och laddade modellers körläge. Visningen uppdateras var femte sekund medan dialogen är öppen. Den väntar medan motorn arbetar.

Modellfiler på disk, nedladdningar, projekt, granskningar och historik raderas inte när modeller lämnar minnet. Modellvalen har samma betydelse som tidigare. Djupare AI-granskning använder fortfarande sitt eget modellval (1,5B), men delar den laddade kopian när övriga textfunktioner väljer samma fil.

## Budget och avvägningar

Minnesuppgifter hämtas från den laddade ggml-motorns enhetsregister genom dess [API för enhetsminne](https://github.com/ggml-org/ggml/blob/master/include/ggml-backend.h). Den installerade versionens CPU-backend använder Windows `GlobalMemoryStatusEx` för tillgängligt fysiskt RAM. Okänd minneskapacitet räknas inte som noll ledigt. På andra operativsystem saknas motsvarande verifiering av tillgängligt RAM i denna implementation.

Textmodellens planering använder den befintliga `llama-cpp-2` 0.1.146-funktionen `fit_params`, med **512 MiB marginal per GPU**, modellvikter, KV-cache och beräkningsbuffertar samt appens fulla kontexttak. Kontexttaket sänks inte för att modellen ska rymmas. Planeringen görs under samma lås som inferensen eftersom API:t ändrar native-bibliotekets globala loggning. Minnesfrågorna slås upp uttryckligen i textmotorns ggml-bibliotek: talmotorn har en separat ggml-version och deras interna enhetspekare får inte blandas. Appen använder native-motorns standardval av enheter och skickar inga sådana pekare mellan motorerna. Om grafikminnet senare understiger marginalen försöker appen först frigöra talmodellen; annars används CPU vid nästa textanrop.

Talmodellens budget är en försiktig uppskattning: två gånger modellfilens storlek plus 512 MiB för buffertar och ytterligare 512 MiB GPU-marginal. Det är ingen exakt Whisper-mätning. Eftersom Whisper och llama har separata enhetsregister används den minsta rapporterade GPU-budgeten vid flera kort; en annan adapters lediga minne får inte räknas som tillgängligt på Whisper-kortet.

CPU-laddning kontrolleras mot tillgängligt fysiskt RAM med marginal. Textmodellens tumregel reserverar filstorleken plus sammanlagt 1 GiB vid CPU eller explicit partiell avlastning. Talmodellens CPU-regel reserverar två filstorlekar plus sammanlagt 1 GiB. Om det inte ryms får användaren ett begripligt fel med förslag om en mindre modell eller att stänga andra program. RAM-tryck definieras som mindre än det största av 1 GiB och tio procent av fysiskt RAM ledigt.

Textmotorns inbyggda enhetsval föredrar separata GPU:er. Appen använder GPU-planering när dessa har rapporterad minnesbudget. På hybriddatorer lämnas den integrerade grafiken utanför textmodellens budget; dess minnestryck tvingar inte ett rymligt separat kort till CPU. Finns ingen lämplig separat GPU används CPU. Whisper kan använda integrerad grafik om både GPU- och RAM-kontrollerna passerar. Detta är medvetet försiktigt och har inte maskinprovats på en kommundator med integrerad grafik. Windows med Vulkan och CPU är de verifierade byggena; Metal och CUDA har inte testats här.

Text- och talinferens köas mot varandra. Det begränsar samtidiga minnestoppar men innebär att exempelvis ett diktat kan få vänta på ett pågående AI-steg. Varje del i en lång textbearbetning släpper låset när delanropet är klart. Varm modellcache behålls på rymliga datorer; efter inaktivitet blir första körningen långsammare eftersom modellen måste laddas igen. CPU-reservläget behålls tills cachen frigörs eller modellvalet byts, så att varje nytt anrop inte upprepar ett misslyckat GPU-försök.

Minnesuppgifterna är ögonblicksbilder och andra program kan använda minne efter kontrollen. Native-biblioteken skiljer inte säkert mellan minnesbrist och andra enhetsfel. Reservvägen omfattar returnerade fel, inte drivrutinskrascher eller native-abort som avslutar processen. För långa instruktioner, ogiltig grammatik och ofullständiga AI-svar utlöser inte CPU-återförsök. Detta är en budget och återhämtningsväg, ingen garanti mot alla former av minnesbrist.

## Verifiering

- Den vanliga Rust-sviten omfattar **81 passerande tester** och 16 opt-in-tester. Nya policytester täcker RAM/GPU-gränser, integrerat minne, okänd GPU, utgången cache, tidsmarginal vid minnestryck och avgränsning av återförsök.
- Verklig Qwen 2.5 3B: två modellreferenser återanvänder samma native-laddning. Ett injicerat kontextfel följs av ett riktigt CPU-svar med **0 av 37 GPU-lager**. Frigöring och återladdning fungerar även genom en redan befintlig modellreferens.
- Verklig budgetplanering med en konstgjord GPU-budget på **2 GiB** ger **29 GPU-lager**, bibehåller **8 192 token** och klarar en kort generering. Testet reserverar resten matematiskt; det fyller inte grafikkortets minne med testallokeringar.
- Verklig KB-Whisper Small: ett injicerat GPU-beräkningsfel följs av riktig CPU-inferens på två sekunders syntetiskt ljud. Resultatets text och segmenttider jämförs med en ny CPU-laddning. Cachen kan frigöras och laddas igen. Detta är ett återhämtningsprov, ingen utvärdering av svensk taligenkänningskvalitet.
- UI-sviterna för arbetsyta 4, 5 och 6 provar befintliga modellval, långa transkript, bevarade utkast vid fel samt ny minnesstatus, pågående arbete, statusfel och stoppad uppdatering när dialogen stängs.
- `npm run check`: noll fel och tre tidigare varningar.
- Både Vulkan- och CPU-bygget klarar de 81 vanliga Rust-testerna. CPU-bygget klarar också de två verkliga cache-/återladdningsproven utan GPU-avlastning. GPU-byggets befintliga kvalitetsprov med långt underlag från arbetsyta 5 passerar på nytt.

Felinsprutningen finns bara i testbinären och kan inte aktiveras i det levererade programmet. Proven använder befintliga lokala modeller och syntetiskt innehåll. Inga möten läses och inga modeller hämtas.

Den tidigare underkända kvalitetskontrollen för långa CPU-frågesvar med Qwen 2.5 3B är **inte åtgärdad** av minneshanteringen. Den finns kvar enligt [arbetsyta 5](ARBETSYTA-STEG-5.md). Ett lyckat CPU-återförsök betyder att beräkningen slutfördes; AI-innehållet behöver fortfarande granskas mot källan.

## Paket

- GPU: `dist/Avskrift-Vulkan-arbetsyta6/avskrift.exe`
- CPU: `dist/Avskrift-CPU-arbetsyta6/avskrift.exe`

Stäng tidigare AVskrift och starta den nya programfilen. Behåll hela paketmappen. Ingen ominstallation behövs. Sidomenyn visar **arbetsyta 6**.
