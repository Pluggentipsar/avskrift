# Genomlysning av mötesflödet i AVskrift

Uppföljning 2026-09-12: genomlysningen nedan beskriver utgångsläget. Levererade ändringar och kvarvarande avgränsningar finns i `ARBETSYTA-STEG-9.md`; aktuella UI-tester finns i `model-tools/ui-step9.cjs`.

Granskat 2026-09-11 mot arbetsytans nuvarande kod. Detta är en genomlysning och ett konkret byggförslag, inte en ny programversion. Produktkoden har inte ändrats i denna granskning.

## Bedömning

Största förbättringen är att låta ett möte vara en sammanhängande arbetsyta från förberedelse till uppföljning. Namn, placering, anteckningar och åtgärder ska finnas kvar när inspelningen stoppas, oavsett hur långt transkriberingen har kommit.

Det finns mycket att återanvända: lokal bearbetning, separata originalspår, bakgrundstranskribering, versionshistorik, mappstruktur, sökning och den gemensamma åtgärdslistan. Problemet är främst hur delarna hänger ihop och några fel i ljud- och sparflödet.

## Din röst hörs men texten saknas

Du uppger att din röst hörs vid uppspelning, men verkar saknas både live och i det sparade transkriptet. Det talar för ett problem mellan inspelning och färdig text. Att mikrofonen får etiketten ”Jag” visar vilken ljudkälla den kommer från; det bevisar inte att ljudet transkriberats korrekt.

Exakt orsak i ditt möte är ännu inte fastställd. Jag har inte lyssnat på eller transkriberat någon privat mötesinspelning. Följande brister finns i koden:

| Steg | Fynd | Konsekvens |
|---|---|---|
| Före live-transkribering | `capture.rs:91,162`: hela ljudblock kastas om toppnivån inte överstiger 0,015, alltså 1,5 procent av full skala. | En låg men hörbar mikrofon kan sparas i WAV-filen utan att nå Whisper. Tröskeln säger inget säkert om huruvida innehållet är begripligt tal. |
| Live-arbetaren | `lib.rs:462`: transkriberingsfel ignoreras med `Err(_) => continue`. Även tomt omsamplat ljud hoppas över. | Ingen tydlig kanalvarning och ingen markering att slutresultatet behöver repareras. |
| Efter stopp | `lib.rs:609`: färdig live-tråd utan kööversvämning räknas som ikapp. | Resultatet kan återanvändas trots att en kanal eller enskilda block saknas. ”Stoppa och transkribera” innebär alltså inte alltid en ny helkörning. |
| Textbaserat ekofilter | `align.rs:64–85`: minst fyra ord, minst 70 procent gemensamma ord, tidsavstånd upp till tre sekunder. | Genuina repliker kan tas bort. Reproducerat mot den faktiska produktionsfunktionen, se nedan. |
| Omtranskribering | `lib.rs:810`: textfiltret körs även när ljudets ekoborttagning är avstängd. | Inställningen ”Ta bort eko ur min mik” ger inte en fullständig väg utan filtrering av mikrofontext. |
| Inläsning av inspelning | `lib.rs:762`: avkodningsfel omvandlas till avsaknad av ljud. | En kanal kan saknas i resultatet utan att användaren får veta att läsningen misslyckades. |

Reproducerat exempel:

1. Mötet, 1,0–4,0 sekunder: ”Vi ska inte boka lokalen nu.”
2. Jag, 4,2–6,0 sekunder: ”Vi ska boka lokalen nu.”
3. Nuvarande textfilter tar bort hela Jag-repliken. Replikerna överlappar inte och säger olika saker.

Detta bevisar ett filterfel, men inte att samma fel förklarar hela ditt saknade mikrofonspår. Om Jag-text aldrig visas live är nivågallringen, transkriberingsfel eller modellens uteblivna igenkänning tidigare i kedjan mer relevanta: text-ekofiltret körs först efteråt. Ljudets adaptiva ekoborttagning är ytterligare en möjlig faktor i helkörningen; den behöver provas med riktig närtalarröst samtidigt som andra talar innan kvaliteten kan bedömas.

### Första tekniska leveransen

- Registrera resultat per kanal: mottagna ljudblock, överhoppade block, fel och producerade textsegment. Logga inte samtalstext för diagnostik.
- Visa en konkret varning om mikrofonljud finns men Jag-text saknas. Skilj ”ljud registrerat” från ”tal identifierat”.
- Reparera misslyckade live-delar från originalspåret efter stopp. Undvik onödiga helkörningar på svaga datorer; börja med felande kanal och reservera helkörning för oklar täckning.
- Ersätt den hårda toppnivågränsen med testad hantering av svagt ljud. Enbart lägre tröskel riskerar mer brus och påhittad text på tystnad.
- Sluta radera genuina repliker enbart på ordlikhet. Behåll osäkra dubletter för granskning, eller kräv starkare stöd från ljudet.
- Låt avstängd ekoborttagning verkligen ge en körning utan båda ekofiltren. Behåll originalinspelningarna.
- Ge ett tydligt fel vid oläsbart spår. Behåll tidigare transkript tills ett nytt resultat har lyckats och godkänts enligt versionsflödet.
- Lägg till uppspelning av ”Båda”, ”Min mikrofon” och ”Övriga”. Det gör felsökning och rättning betydligt enklare.

Godkännandekrav: svag begriplig röst, tystnad, kort slutsvar, headset, högtalare med eko och samtidiga talare. Kontrollera text och tidsstämplar per kanal samt återhämtning efter ett framkallat transkriberingsfel. GPU och CPU ska följa samma regler. En verklig drabbad inspelning behövs för att bekräfta just ditt fall.

## Bekräftade problem i användarflödet

| Prioritet | Problem | Föreslagen lösning |
|---|---|---|
| 1 | Anteckningar under inspelning sparas inte löpande: `saveWorkspace` avbryter när `meetingActive` är sant. | Skapa mötets identitet vid start och spara anteckningar/åtgärder fortlöpande. Återställ dem vid omstart. |
| 1 | Stopp nollställer arbetsvyn och skickar användaren till förberedelse för nästa möte. | Stanna i samma möte med status ”Transkriberar”. Ha ”Nytt möte” som separat handling. |
| 1 | Ett väntande möte visar en laddningsvy även på fliken Anteckningar och åtgärder. | Lås endast de funktioner som kräver transkript. Anteckningar, deltagare, namn och åtgärder ska vara tillgängliga. |
| 2 | Namnet är passiv text i mötets sidhuvud. Byte kräver bibliotekets meny. | Synlig ”Byt namn”-knapp vid rubriken, redigering med Enter för spara och Escape för avbryt. Samma funktion i Ditt arbete. |
| 2 | Namn skapas automatiskt först vid stopp. Ingen enkel förberedelse med namn eller agenda. | Namnge före start med datum som förval. Återanvänd samma namn under inspelning och bearbetning. |
| 2 | Inspelningsvyn visar inga individuella ljudnivåer eller enhetsval. | Visa aktuell mikrofon/utgång, två nivåmätare och ett kort ljudtest. Beskriv tydligt vad som mäts. |
| 2 | ”Möten” går till inspelningsstart även när ett möte redan är öppet. | Mötesingång med senaste möten och tydlig väg tillbaka till det aktuella mötet. |
| 2 | Återöppning väljer sammanfattning om ett utkast finns, annars transkript. | Kom ihåg senast använda mötesflik och läsposition. |
| 3 | Verktyg, modellval och instruktioner tar en hel sidokolumn i anteckningsvyn. | Flytta relevanta åtgärder till respektive sektion och samla avancerade val i en utfällbar panel. |
| 3 | Deltagare och uppföljningsdatum konkurrerar med själva skrivytan. | Placera mötesfakta i Översikt. Behåll anteckningsytan bred och låt detaljer öppnas vid behov. |

Ett extra tydlighetsfel: texten för ett väntande transkript säger att appen kan stängas eftersom ljudet är sparat. Det ska framgå att den pågående bearbetningen då avbryts och måste återupptas, inte att den fortsätter efter stängning.

## Föreslagen mötesyta

```text
Arbetslaget / Höstens planering
Planeringsmöte 11 september     [Byt namn] [Flytta] [Mer]
45 min · Transkriberar din röst · Anteckningar sparade

Översikt | Transkript | Anteckningar | Beslut och åtgärder

Innehåll för vald vy

[Spela / pausa] [Båda spåren ▾] ─── position ─── [Hastighet]
```

Översikt innehåller mötesfakta, deltagare, kort granskad sammanfattning, öppna åtgärder och nästa uppföljning. Den visar nästa användbara handling utifrån mötets tillstånd. Transkript behåller nuvarande sökning, rättning och uppspelning. Anteckningar får större skrivyta och möjlighet att lägga in en tidsmarkör. Beslut och åtgärder skiljer på vad som beslutades och vem som ska göra vad.

Sammanfatta, fråga källan, avidentifiera och exportera finns kvar som tydliga verktyg i mötet. De är valfria arbetsmoment; alla möten behöver inte gå igenom varje moment. Möten, diktering och avidentifiering behåller sina tre huvudingångar.

Före inspelning: namn, placering, valfritt syfte/agenda och ljudkontroll. Under inspelning: stabil rubrik, tydlig stopphandling, kanalstatus och anteckningar. Efter stopp: samma möte och samma anteckningar medan texten färdigställs. Vid återbesök: öppna där användaren slutade.

## Ditt arbete och organisering

Gör Ditt arbete till en praktisk startsida med fyra avsnitt:

1. **Pågår nu:** aktiv inspelning och möten som bearbetas, med namn och direktlänk.
2. **Fortsätt arbeta:** senast öppnade arbeten, fästa favoriter och senast använda vy.
3. **Att följa upp:** förfallna/närliggande åtgärder och mötens uppföljningsdatum.
4. **Börja nytt:** de tre befintliga ingångarna, kompletterade med import av ljud/text.

Låt varje möte vara en enda post som äger ljud, transkript, anteckningar, deltagare, beslut, åtgärder och utkast. Ett möte ska inte bli flera likvärdiga poster för varje bearbetningssteg.

Återanvänd befintliga mappar som arbetsområden, exempelvis Arbetslaget eller Projekt X. Lägg till filter för typ och tillstånd samt Fästa och Arkiverade. Behåll djupare mappar för den som behöver dem, men kräv ingen mappstruktur för att börja arbeta. Vyer och sökträffar ska peka på samma möte; de ska inte skapa kopior.

Använd konsekventa begrepp: Ditt arbete = startsidan, Bibliotek = samtliga sparade arbeten, möte/dokument/diktat = typen av arbete, mapp = placering. Nu blandas bland annat Alla projekt, Mina projekt, historik och jobb. Byt ut äldre texter samtidigt.

Varje rad bör visa namn, typ, placering, senast öppnat/ändrat och relevant status, till exempel ”Transkriberar”, ”3 öppna åtgärder” eller ”Uppföljning 18 sep”. Lägg ”Byt namn”, ”Flytta”, ”Fäst” och ”Arkivera” i samma meny överallt. Visa inte ett tidigare projekts mapp och versionsknapp i startsidans sidhuvud utan dess namn, som sker idag.

Åtaganden finns redan och har ansvarig, datum och filter. Utveckla den befintliga listan med länkar tillbaka till rätt möte och exakt punkt. Skapa inte en konkurrerande uppgiftslista.

## Funktioner med störst nytta efter grundflödet

- Tidsmarkerade anteckningar: ”Markera här” under mötet och ”Lyssna på sammanhanget” efteråt.
- Beständiga beslut med källa. En godkänd AI-punkt kan bli beslut eller åtgärd med bevarad koppling till källan.
- Bevara källhänvisning på åtgärder: `addGroundedAction` kopierar idag endast texten till åtgärdslistan.
- Återanvändbar mötesmall för agenda, deltagarroller och sammanfattning.
- ”Förbered uppföljningsmöte”: välj öppna punkter från föregående möte utan att kopiera hela historiken eller ändra originalet.
- Samlad export där användaren väljer sammanfattning, beslut, åtgärder, anteckningar och eventuellt transkript, med tydlig förhandsgranskning.

## Rekommenderad byggordning

| Leverans | Innehåll | När är den klar? |
|---|---|---|
| A – Tillförlitligt möte | Kanaldiagnostik, reparation från originalspår, ekofilter, löpande anteckningssparning. | Testat på tyst/svagt ljud och framkallade fel. Anteckningar kan återställas efter avbrott. Din drabbade inspelning bekräftar eller avgränsar mikrofonfelet. |
| B – Sammanhängande mötesvy | Namn före/under/efter mötet, inline byte, stabil identitet vid stopp, anteckningar åtkomliga under bearbetning, kom ihåg vald vy. | Hela start–stopp–återöppna-flödet fungerar utan att kontext eller redigeringar förloras. |
| C – Ditt arbete | Pågår, Fortsätt, uppföljning, enhetliga menyer/begrepp, fäst/arkivera och filter. | Samma möte hittas och hanteras på samma sätt från startsida, bibliotek och åtgärdslista. |
| D – Mötesuppföljning | Tidsmarkerade anteckningar, beständiga beslut/källor, nästa möte och samlad export. | Varje beslut/åtgärd går att följa tillbaka till sitt underlag och vidare till nästa möte. |

B kan delvis förberedas samtidigt med A, men saknad röst och utebliven sparning ska inte döljas av en visuell ombyggnad.

Tekniskt bör mötessession, sparning/bakgrundsslutförande och mötesvyer få separata moduler. Nu ligger stora delar i `+page.svelte`. Behåll befintliga projekt-ID:n och formatkompatibilitet. Bakgrundsresultat ska bara uppdatera transkript/ljudstatus och inte skriva över nyare namn, anteckningar eller åtgärder. Testa särskilt att ett mycket kort möte hinner färdigställas före sparningen av det väntande projektet; nuvarande ordning kräver en separat kontroll av den kapplöpningen.

## Underlag och avgränsning

- Kodgranskning av `capture.rs`, `transcribe.rs`, `aec.rs`, `align.rs`, möteskommandon i `lib.rs`, huvudvyer och navigering.
- Visuell körning med syntetiska mötesdata i Edge, 1440×960 och anteckningsvy vid 900×700. Inga verkliga ljudenheter aktiverades.
- `model-tools/meeting-flow-audit.cjs` gav: `savedDuringRecording=false`, `pendingNotesVisible=0`, anteckningen sparad vid stopp, inga JavaScript-fel.
- `model-tools/meeting-echo-audit.rs` kör produktionsfunktionen i `align.rs` och reproducerar den felaktigt borttagna repliken. Det är en reproduktion av nuvarande fel, inte ett test som godkänner beteendet.
- Skärmbilder och maskinläsbara observationer finns i `docs/meeting-flow-audit/`.
- Ingen ny CPU-/GPU-version har byggts. Ingen modellkvalitet, faktisk mikrofonvolym eller verklig inspelningssynkronisering har mätts i denna genomlysning.
