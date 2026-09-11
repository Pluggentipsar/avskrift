# Diktering i AVskrift

Första versionen är för Windows. Öppna **Diktera**, välj en nedladdad talmodell och aktivera
kortkommandona. Minimera AVskrift och sätt markören i ett textfält. AVskrift måste vara igång.

- **Ctrl+Shift+Space:** håll inne medan du talar. När någon av tangenterna släpps stoppas
  inspelningen och transkriberingen börjar, även om du släpper mycket snabbt.
- **Ctrl+Alt+Space:** tryck en gång för att starta utan att hålla inne. Tryck igen för att
  stoppa och transkribera. Att släppa tangenterna stoppar inte detta läge.

Båda kortkommandona är aktiva samtidigt. Om en genväg är upptagen visas vilken; den andra
kan fortfarande användas. Ett nytt hålltryck ignoreras medan ett diktat spelas in eller
bearbetas. Släpphändelsen hör till det diktat som startades av hålltrycket och kan inte
stoppa ett senare diktat. Knappen i AVskrift kan alltid stoppa en pågående inspelning.

En indikator visas utan att ta tangentbordsfokus. Knappen **Starta diktat här** samlar text
i AVskrift och försöker inte infoga i ett annat program. Ljudet transkriberas efter stopp;
ord visas ännu inte löpande under inspelningen. Ett diktat är högst fem minuter.

## Text, återhämtning och historik

- Alla färdiga diktat finns i vyn innan automatisk infogning försöks.
- Diktat kan sökas, redigeras, kopieras och öppnas för avidentifiering. **Behåll ändring**
  sparar en textändring enligt diktatets befintliga lagringsval.
- Utan **Spara diktat** eller **Spara nya diktat automatiskt** finns texten bara under
  appens session. Den försvinner när appen avslutas. Ljudet skrivs aldrig till en fil.
- Sparad text och inställningar ligger i `dictation.json` i appens datakatalog. Historiken
  skrivs via en tillfällig fil och ersätts efter avslutad skrivning. En oläsbar historik
  lämnas orörd och ett fel visas. Sparfel behåller den nya texten i minnet.
- **Behåll bara i sessionen** tar bort just det diktatet från den sparade historiken.
  Att stänga av automatisk lagring påverkar bara nya diktat. **Ta bort** tar bort ett diktat.

## Infogning och begränsningar

Windows UI Automation används för att kontrollera det fokuserade textfältet vid start och
jämföra samma element före infogning. Lösenordsfält, kända skrivskyddade fält, AVskrifts egna
fönster och mål som inte kan kontrolleras får ingen automatisk text. Fokus återställs aldrig
automatiskt. Vid ändrat fokus finns texten kvar för manuell kopiering.

Infogningen använder Unicode-tangentbordshändelser, vilket lämnar hela det befintliga
urklippet orört. Retur/tabb skickas inte, så diktering ska inte skicka formulär eller byta
fält. **Infogning skickad** betyder att Windows tog emot händelserna; målprogrammet kan
ändå ignorera dem. Kontrollera resultatet innan du kopierar igen. Program som körs med
högre behörighet och vissa egna textredigerare kan kräva manuell kopiering.

Den lokala talmotorn delas med filtranskribering. Ett pågående jobb får bli klart först.
Modellen och dess arbetsminne återanvänds. När kortkommandona aktiveras förbereds modellen
i bakgrunden; vyn visar när detta pågår. Första GPU-starten kan ta extra tid för shaderkompilering.
Vyn visar också om bygget har GPU-stöd. Se [prestanda](PRESTANDA.md) för optimerade Windows-paket.
Diktering och mötesinspelning kan inte startas samtidigt. Avbryt stoppar mikrofonen direkt;
en redan påbörjad Whisper-körning får avslutas innan arbetsläget blir ledigt, men resultatet
infogas inte. Stängning av huvudfönstret stoppas under pågående diktering.

## Verifiering inför användning

Automatiska tester täcker lagring av enbart sparade diktat, ersättning av befintlig historik,
bevarande av korrupt historik, tystnad/klickljud och Unicode-händelser för svenska tecken
och surrogatpar. De täcker även kortkommandolägen, snabb släppning under uppstart, fördröjd
släppning från ett äldre diktat och läsning av tidigare sparade inställningar.
Kör `cargo test --release --lib dictation` från `src-tauri`.

Manuell kontroll på en Windowsdator:

- Diktera en kort svensk mening i Anteckningar, ett webbläsartextfält och Word.
- Starta utan textfält, i ett lösenordsfält och i ett skrivskyddat fält: återhämta från Diktat.
- Byt fönster eller textfält under inspelningen: ingen automatisk infogning i det nya målet.
- Kontrollera att kortkommandot fungerar medan AVskrift är minimerat, och att indikatorn
  inte tar fokus. Prova även krock med en annan registrerad genväg.
- Håll Ctrl+Shift+Space och släpp i tur och ordning Space, Shift eller Ctrl först: transkribering
  ska börja utan ett andra tryck. Ctrl+Alt+Space ska fortsätta spela in när tangenterna släpps.
- Avbryt under inspelning och transkribering. Koppla ur mikrofonen under inspelning.
- Spara ett diktat, starta om och återöppna. Ett tillfälligt diktat ska inte finnas efter omstart.
- Kopiera en bild före diktering: automatisk infogning ska lämna urklippet oförändrat.

SagaScript inspirerade arbetsflödet. Ingen SagaScript-kod har kopierats in; funktionen använder
AVskrifts befintliga KB-Whisper-motor och Windows-API:er.
