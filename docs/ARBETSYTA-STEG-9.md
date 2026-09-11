# AVskrift – arbetsyta 9

Möten från inspelning till uppföljning. Byggd 12 september 2026.

## Starta versionen

- Med separat grafikkort, inklusive RTX 5070 Ti: `dist/Avskrift-Vulkan-arbetsyta9/avskrift.exe`.
- Utan GPU-stöd: `dist/Avskrift-CPU-arbetsyta9/avskrift.exe`.

Stäng den äldre versionen, även i aktivitetsfältets meddelandefält, och starta den nya programmappen. Ingen ominstallation behövs. Flytta hela mappen om du vill placera programmet någon annanstans; DLL-filer och resurser måste följa med. Befintliga arbeten och hämtade modeller ligger kvar i användarens appdata. Bibliotekets sökindex återskapas automatiskt vid behov.

## Nytt mötesflöde

1. Öppna Möten. Ange namn och valfri agenda. Välj mikrofon och ljudutgång, eller använd Windows standardenheter. Använd samma mikrofon som du talar i under själva mötet.
2. Knappen **Testa ljud i 3 sekunder** mäter mikrofon och mötesljud. Tala och spela upp ljud under testet. Testet mäter ljudnivå, inte om orden kan transkriberas. Testfilerna tas bort efter testet.
3. Starta inspelningen. Anteckningar och åtgärder sparas löpande; **Markera här** skapar en tidsmarkerad anteckning. Två nivåmätare visar vilka kanaler som tar emot ljud.
4. Stoppa inspelningen. Samma möte öppnas i Översikt. Anteckningar, beslut, deltagare och åtgärder är åtkomliga medan transkriptet bearbetas.
5. Öppna Transkript för rättning och uppspelning. Välj **Båda spåren**, **Min mikrofon** eller **Övriga deltagare** och justera uppspelningshastigheten.
6. Skapa ett källbelagt utkast under Sammanfattning. Granskade förslag kan läggas till som beslut eller åtgärder med bevarat källutdrag.
7. **Exportera mötesunderlag** låter dig välja agenda, sammanfattning, beslut och anteckningar/åtgärder. Originaltranskriptet kan bifogas. Förhandsgranska innan du sparar text eller Word-fil.

Byt namn direkt vid rubriken. **Hantera** har namnbyte, flytt, fästning och arkivering i mötet, på startsidan och i biblioteket. Arkivering flyttar arbetet ur den aktiva bibliotekslistan; det raderar inga ljud eller texter.

## Ditt arbete och uppföljning

- **Pågår nu** visar inspelning och möten som bearbetas, med direktlänkar.
- **Fortsätt arbeta** prioriterar fästa arbeten och senast öppnade arbeten.
- **Att följa upp** visar daterade öppna åtgärder och möten med uppföljningsdatum.
- **Bibliotek** har filter för aktiva, fästa och arkiverade arbeten samt innehållstyp. Sökningen omfattar även mötesagenda, beslut och tidsmarkerade anteckningar.
- Möten återöppnas i senast använda flik. Uppspelningspositionen sparas vid paus eller när du lämnar arbetsvyn.
- **Förbered uppföljningsmöte** skapar ett förberett nytt möte. Välj vilka öppna åtgärder som ska följa med. Det föregående mötet ändras inte och går att öppna via en länk.
- Egna mötesmallar kan innehålla agenda, deltagare/roller och val av sammanfattningsmall.

## Rättningar för mikrofontext

Den tidigare live-gränsen kastade hela ljudblock under toppnivån 0,015. Nu används uthållig ljudenergi i korta ramar och begränsad förstärkning av svagt mikrofonljud. Originalinspelningen förstärks inte. Detta är en ljudaktivitetskontroll, inte en modell som säkert skiljer tal från brus.

Fel och tomma resultat från aktiva live-block markeras för återhämtning. Vid ofullständig live-transkribering används originalspåren för ett nytt försök efter stopp. Återhämtningen gör idag en samlad körning av båda kanalerna; en mer selektiv omkörning av bara en kanal är en framtida prestandaförbättring.

Det textbaserade ekofiltret har tagits bort. Det kunde radera genuina svar enbart för att de innehöll samma ord som den andra talarens replik. Ljudbaserad ekoborttagning vid omtranskribering är valfri och avstängd från början. Med högtalare kan dubblerade repliker därför förekomma; hörlurar och granskning är fortfarande användbara.

Oläsbara ljudspår och inspelningsfel ska ge felmeddelanden i stället för ett tyst bortfall. Ett möte utan Jag-text visar en uppmaning att kontrollera mikrofonspåret. Modellens tidsstämplar begränsas till det verkliga ljudblockets längd; texten behålls även när modellen placerat en tid i sin interna utfyllnad.

För ett äldre drabbat möte: öppna Transkript, visa verktygen och välj **Kör om med vald modell**. Börja med ekoborttagning avstängd. Testerna bekräftar att en tidigare bortfiltrerad typ av svagt ljud nu når Whisper. Exakt orsak i användarens äldre möte är ännu inte verifierad mot den inspelningen.

## Sparning och återhämtning

Mötet får sitt projekt-ID och sina ljudsökvägar redan vid start. WAV-headern uppdateras ungefär en gång per sekund så att filen kan läsas fram till senaste lyckade uppdatering efter ett avbrott. Anteckningar sparas med den befintliga fördröjningen på ungefär en halv sekund. Detta är inte en garanti mot förlust vid strömavbrott eller diskfel.

Bakgrunden sparar färdigt transkript och ljudstatus direkt i projektet under lagringens lås. Den skriver inte över namn eller anteckningar. En sen autosparning från inspelningsvyn får inte återställa ett redan färdigt transkript till väntande läge. Stängning av appen stoppar först en aktiv inspelning; bearbetning som avbryts när appen avslutas kan köras om från de sparade ljudspåren.

## Verifiering

- Svelte/TypeScript: inga fel. Tre tidigare varningar återstår: två äldre dialogers fokusattribut och Node-typdefinitioner.
- 101 ordinarie Rust-tester på CPU och GPU. 20 tester är opt-in och ingår inte i det ordinarie testantalet.
- Ett separat opt-in-test med syntetiskt tal och riktig KB-Whisper-small kördes på både CPU och RTX 5070 Ti/Vulkan. Hela talfilen dämpades till toppnivå 0,004, under den gamla live-gränsen. Förväntade ord och begränsade mötestidsstämplar verifierades.
- UI-regressioner: sparning/versioner, källgranskning, avbrytning, bibliotekssökning och hela mötesflödet i `ui-step2`, `ui-step3`, `ui-step7`, `ui-step8` och `ui-step9`.
- Nytt lagringstest täcker sent inspelningsutkast efter färdig transkribering, bevarade anteckningar/namn/fästning, sökning i nytt mötesinnehåll och källhänvisningar efter åtgärdsredigering.
- UI-testet använder syntetiska data och täcker inspelningens autosparning, namnbyte, markeringar, stopp, bakgrundsresultat mitt i en redigering, exporturval, uppföljning, arkivering och återöppning. Bilder finns i `docs/ui-step9/`.

Ingen privat mötesinspelning har använts. Fysisk inspelning med olika headset, högtalare och enhetsbyten är inte automatiskt slutprovad i denna leverans. Det tidigare kända kvalitetsproblemet för extremt långa Qwen-frågeunderlag på CPU är inte åtgärdat av mötesändringarna.

## Implementering

`meeting.rs` samlar ljudaktivitetskontroll, förstärkning, live-täckning och tidsgränser. `capture.rs` hanterar enhetsval, nivåer och återställbara WAV-filer. Möteskommandon och atomiskt slutförande ligger i `lib.rs`/`jobs.rs`. Bibliotekets cacheformat uppdateras till version 2. Nya projektfält lagras kompatibelt via projektets befintliga extra-fält; åtgärder bevarar också extra-fält som källhänvisning och ID.

Gränssnittet behåller AVskrifts vita/ljust violetta palett, Archivo för arbetsytan och Instrument Serif för rubriker. Översikt och skrivytor använder hela bredden. Avancerade val ligger vid relevant innehåll; samma möte har samma namn och identitet genom hela flödet.
