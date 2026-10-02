# Pianissimo i Avskrift

Pianissimo svenska är ett experimentellt, valbart CPU-alternativ för ljudfiler,
möten och diktering. KB-Whisper är fortfarande standard. Tidigare sparade
modellval behålls. Modellvalen för möten/ljudfiler och diktering är separata.

Öppna **Modeller på datorn**, välj **Pianissimo svenska — CPU (experimentell)**
och välj **Hämta modell**. Cirka 923 MB hämtas från den låsta moonhouse-exporten.
Varje fil kontrolleras med SHA-256. Därefter förbereds modellen lokalt; detta
kan ta flera minuter första gången. Status visas i modellfönstret. Python
behövs varken för förberedelse eller transkribering.

Modellen sparas i appens datakatalog under `pianissimo-sv`. Originalfiler och
förberedd modell tar cirka 1,8 GB; under förberedelsen behövs cirka 2,7 GB.
Knappen **Kontrollera modell** verifierar filerna och återskapar en skadad cache.
Efter ändrad ONNX Runtime eller CPU behöver modellen förberedas på nytt.

## Funktion och begränsningar

Minneskontrollen för Pianissimo använder på Windows både ledigt RAM och
operativsystemets återstående utrymme för minnesreservationer (commit headroom,
inklusive växlingsfil). Den tidigare kontrollen krävde 4,5 GiB ledigt fysiskt RAM
enbart för förberedelsen, vilket blockerade datorer som kunde använda växlingsfil.
Nu krävs 4,5 GiB reservationsutrymme vid förberedelse, 2,5 GiB vid laddning och
minst 256 MiB ledigt RAM. RAM adderas aldrig till commit-uppgiften, eftersom
det då skulle räknas två gånger. Vid lågt RAM visar förloppet att arbetet kan ta
längre tid. GPU- och övriga modellers minnesregler är oförändrade.

En redan färdig, verifierad cache behöver ingen ny förberedelse och kontrolleras
därför utan förberedelsens stora minneskrav. Hämtade filer återanvänds efter
ett avbrutet försök, förutsatt att deras kontrollsummor stämmer.

- Endast svenska; inget översättningsläge. Dessa inställningar spärras även i backend.
- Ungefärliga segmenttider för uppspelning, talartilldelning och undertexter.
  Modellen ger tokenstarter, inte verifierade ordslut. Ordmarkering är därför avstängd.
- Längre ljud bearbetas i avsnitt på 28 sekunder med två sekunders extra ljud
  på varje sida. Ljud upp till 36 sekunder körs i ett stycke. Ett ord hör till det avsnitt där dess första token börjar.
  Kontrollera texten särskilt nära avsnittsgränser och vid talarbyten.
- Avbrytning kontrolleras före ljudbearbetning, mellan modellens tolv delar,
  under avkodning och mellan ljudavsnitt. Ett pågående enskilt ONNX-anrop eller
  modelladdning avslutas innan avbrytningen får effekt.
- Den laddade modellen återanvänds och frigörs efter inaktivitet eller minnestryck,
  enligt samma princip som de befintliga talmodellerna.
- Naturliga inspelningar, flera verkliga talare, brus och andra datorer behöver
  fortfarande kvalitetsbedömas innan ett eventuellt byte av standardmodell.

## Implementation och kontroller

`crates/pianissimo` innehåller den gemensamma Rust-motorn. Prototypverktyget
använder samma motorfiler. Appadaptern i `src-tauri/src/pianissimo.rs` hanterar
hämtning, modellcache, minne och avbrytning. Befintliga transkriberingsanrop
för ljudfiler, möten och diktering väljer motor utifrån modell-id.

Förberedelsen optimerar hela originalgrafen med `session.x64quantprecision=1`
och delar därefter den redan optimerade grafen i tolv delar. Vikter och ONNX-fält
kopieras utan ny kvantisering; interna typangivelser bevaras. Ytterligare
grafoptimering stängs av för delarna. Encoder-delarna har gemensam trådpool
och separata minnesarenor avstängda. NER behåller en oberoende trådpool.

Lokalt 2026-09-25, Ryzen 7 7730U, Windows, CPU:

- Åtta enhetstester för modell, ljudfunktioner, ONNX-inläsning och segmentering passerade.
- Rust-förberedelsen tog 102,07 s; efterföljande modelladdning 6,34 s. Enstaka
  mätning med varm filcache, ingen generell prestandagaranti.
- Fem syntetiska ljudprov gav exakt samma text, token-id, tokensträngar och
  tokenstarter som den tidigare verifierade Rust-prototypen, inklusive tom text för tystnad.
- Slutliga textsegment återgav samma text för dessa korta prov; tiderna låg
  inom ljudfilen och överlappade inte. Avbrytning och återanvändning passerade.
- Ett längre syntetiskt prov (142,36 s) visade att den låsta exporten tappar
  mycket tal med 60-sekundersavsnitt. Därför begränsas varje inferens till
  högst 36 sekunder och långa ljud delas i kortare avsnitt. Numerisk likhet
  med originalexporten räcker inte som kvalitetskontroll.
- Encoderjämförelse vid 1,0, 41,01 och 71,18 s mot sparade originalresultat:
  exakt lika värden, maximalt absolut fel 0 och relativt L2-fel 0.
- Frontendens typkontroll: inga fel; tre befintliga varningar.
- Slutligt långprov, 142,36 s med 28-sekundersavsnitt: 56 segment och alla
  316 förväntade ord i rätt ordning. Ingen skillnad efter normalisering av
  versaler och skiljetecken. Samma syntetiska stödtext upprepades fyra gånger;
  detta är ett regressionstest, inte ett naturligt långt möte.
- Appens två riktade tester passerade: ljudavkodning, faktisk transkriberingsväg,
  språkspärr, avbrytning/återanvändning, riktig sherpa-diarisering med en talare,
  talartilldelning och SRT/VTT-export. Detta verifierar inte kvalitet på flera röster.
- Windows CPU-release byggd och startad. Modellväljaren visade Pianissimo för
  både möten och diktering; sparat val `kb-whisper-medium` behölls i UI-testet.
  UI-kontrollen var läsande. Windows använde ordinarie appdata trots processens
  APPDATA-variabel, så inga modellval ändrades under testet.

Lokalt testbygge med minnesrättningen: `dist/Avskrift-CPU-pianissimo-memory-fix/avskrift.exe`. Hela katalogen
behövs, inklusive medföljande DLL-filer och resurser. Modellen hämtas separat
via modellinställningarna. Ingen installerad app har ersatts.

Utvecklartest med egna uttryckligen valda lokala PCM16/16 kHz-filer:

```powershell
cargo test --manifest-path crates/pianissimo/Cargo.toml
cargo run --release --manifest-path crates/pianissimo/Cargo.toml --example verify -- MODEL_DIR REPORT.json WAV...
```

Testet `pianissimo::tests::app_pipeline` är ett separat ignorerat apptest och
läser `AVSKRIFT_PIANISSIMO_TEST_MODEL` och `AVSKRIFT_PIANISSIMO_TEST_AUDIO`.
Det kräver lokala diariseringsmodeller. Inga testfiler eller ljud laddas upp.
Lokala rapporter ligger under `.build-tools/pianissimo/app-*.json` och checkas inte in.

## Modellens upphov och licens

Originalmodell: [KlangAI/Pianissimo](https://huggingface.co/KlangAI/pianissimo-sv).
INT8-export: [moonhouse/pianissimo-sv-onnx](https://huggingface.co/moonhouse/pianissimo-sv-onnx),
revision `72c38267654dadd538bceac7a851de00fb55f11a`.
Modellen omfattas av [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Avskrift skapar en lokalt optimerad, uppdelad variant för snabbare modellstart.
Se även [NOTICE](../crates/pianissimo/NOTICE.md) och
[tidigare mätningar](PIANISSIMO-PROTOTYP.md).

## Verifiering av minnesrättningen, 2026-09-25

Två riktade tester av minnesbudgeten passerade, inklusive 900 MiB ledigt RAM
med tillräckligt commit-utrymme, otillräckligt commit-utrymme, RAM-reserv,
reservvägen utan commit-uppgift och skydd mot dubbelräkning.

Ett fullt apptest förberedde modellen från originalfiler med cirka 2,57 GiB
ledigt RAM vid teststart, under den tidigare gränsen på 4,5 GiB. Förberedelse,
återanvändning av cache utan ny minnesreservation och inferens på tystnad
passerade på 132,79 s. Högsta working set var 2 640 687 104 byte (2,46 GiB),
högsta process-commit 2 643 951 616 byte. Under mätningen sjönk ledigt RAM
till 425 177 088 byte (cirka 405 MiB). Ingen ny modellnedladdning gjordes.

Den förberedda cachen kopierades därefter till den lokala appens modellkatalog.
Originalfiler och samtliga kopierade delar kontrollsummerades. Användarens
öppna app och sparade modellval ändrades inte. Den rättade versionen måste
startas för att använda de nya minnesreglerna vid transkribering.
