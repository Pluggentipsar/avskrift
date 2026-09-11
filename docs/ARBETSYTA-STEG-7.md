# Arbetsyta 7 – mindre ljudbuffertar och avbrytbara arbeten

2026-09-11. Bygger vidare på arbetsyta 6.

## Vad som ändrats

- Ljudfiler avkodas och omsamplas paketvis till 16 kHz mono. Hela ljudfilen ligger inte längre samtidigt i en extra monobuffert med originalets samplingsfrekvens. För en timme vid 48 kHz försvinner därmed en mellanbuffert på cirka **659 MiB**, räknat på 32-bitars flyttal. Det är en beräkning av den borttagna bufferten, inte en mätning av programmets totala minnestopp.
- Omsamplaren behåller filterhistoriken mellan paketen, tömmer slutet och lämnar exakt den avrundade längden vid 16 kHz. Dekoderpaketens faktiska samplingsfrekvens och kanaler används. En ändring av ljudformat mitt i filen ger ett begripligt fel.
- **Avbryt arbete** finns vid filtranskribering, sammanfattning, frågor, åtgärdsförslag, källutkast, diktatbearbetning och avidentifiering. Under avbrytningen visas att motorn håller på att stanna. Tidigare transkript, granskningar och utkast ersätts först när det nya arbetet har slutförts.
- Ett arbete registreras före start och får ett eget ID. Avbrytning före start gäller också; sena avbrott kan inte träffa nästa arbete. När resultatet redan publicerats visas att arbetet hann bli klart.
- Whisper kan avbrytas via sin native-callback. En avbruten Whisper-allokering frigörs efter att native-anropet återvänt, så nästa diktat får ett rent tillstånd. Qwen kontrollerar avbrytning mellan inläsningsbatcher och genererade token. Avbrytning utlöser inget nytt CPU-försök.
- Diktering får företräde framför vanliga arbeten som väntar på modellmotorn. Pågående native-beräkning äger motorn tills steget är klart. Förvärmning av talmodellen hoppar över försöket om motorn redan är upptagen.

All bearbetning sker fortsatt lokalt. Avbrytning raderar inte projekt, original, hämtade modeller eller sparade ljudfiler.

## Begränsningar

Det färdiga 16 kHz-ljudet finns fortfarande i en sammanhängande buffert: cirka 220 MiB per timme, före kapacitetsmarginaler. Diarisering, ekoreducering och mötesmixning kan skapa fler kopior. Detta tar bort en stor mellanbuffert, men ger inte konstant minnesförbrukning för hela mötesflödet.

Modelladdning och en pågående ONNX-körning kan behöva slutföras innan avbrytningen märks. Diarisering som ingår i en filtranskribering kontrollerar avbrott före och efter native-steget. Den nya knappen omfattar inte separat **Transkribera om mötet**, separat talaruppdelning eller automatisk mötesslutföring i bakgrunden; de flödena har egna ljud- och filskrivningar och behöver en separat lösning för avbrott och återupptagning.

Diktering avbryter inte automatiskt en pågående filtranskribering eller AI-generering. Den går före vid nästa lediga modellsteg, till exempel mellan delar av en lång sammanfattning. Behövs motorn tidigare kan användaren avbryta det pågående förgrundsarbetet.

Den tidigare underkända kvalitetskontrollen för långa CPU-frågesvar med Qwen 2.5 3B är inte åtgärdad här. Se [arbetsyta 5](ARBETSYTA-STEG-5.md). Avbrottsproven verifierar körning och återhämtning, inte svensk taligenkänningskvalitet eller AI-svarens saklighet.

## Verifiering

- 89 vanliga Rust-tester passerar i CPU- och Vulkan-konfiguration. 18 tester är uttryckliga opt-in-prov.
- Ljudtester täcker 8, 16, 44,1, 48 och 96 kHz, olika paketstorlekar, korta/exakta/ojämna längder, stereo-WAV, kanalmedelvärde, blockgränser och svans. Impulser visar högst ett sampel i tidsavvikelse vid start, blockgränser och slut.
- Den installerade rubato 0.15-implementationens SincFixedIn kompenserar redan startindexet. `output_delay()` får därför inte användas för att skära bort ytterligare utdata här; impulstesterna skyddar mot en sådan felaktig trimning.
- Kötester verifierar att diktering går före vid frigivning, att aktiv inferens inte släpps och att avbrutna väntande arbeten inte låser kön. Avbrottstester täcker avbrott före dispatch, resultatpublicering, nästa arbetes isolering och städning vid panic.
- Verklig Qwen 2.5 3B och KB-Whisper Small avbryts med syntetiska underlag på både GPU och CPU. Därefter lyckas en ny körning. Ingen CPU-återladdning sker till följd av avbrottet; avbrutet Whisper-tillstånd kastas.
- UI-prov täcker avbrottsknapp, misslyckad avbrottsbegäran och nytt försök, väntan tills motorn svarar, redan slutfört arbete, bibehållna utkast, avbrytning inne i diktatdialogen filtranskribering med avbrott/nytt försök och skydd mot projektbyte under arbete. Befintliga UI-sviter för arbetsyta 3–6 passerar.

## Starta nya versionen

- GPU: `dist/Avskrift-Vulkan-arbetsyta7/avskrift.exe`
- CPU: `dist/Avskrift-CPU-arbetsyta7/avskrift.exe`

Stäng tidigare AVskrift och starta den nya programfilen. Behåll hela paketmappen. Ingen ominstallation behövs. Sidomenyn visar **arbetsyta 7**.
