# Pianissimo: lokal genomförbarhetsprototyp

Undersökning 2026-09-25. Nedan dokumenteras prototyparbetet och dess mätningar.
Pianissimo har därefter kopplats in som ett experimentellt modellval i appen;
se [appintegrationen](PIANISSIMO.md). KB-Whisper är fortfarande standard och
användarens sparade modellval ändras inte.

## Senaste resultat: optimering före uppdelning

**Modellstarten i Rust är 7,84 s efter lokal engångsförberedelse**, mot
131,08 s för den sammanhängande exporten: cirka 94 procent kortare tid.
Det fungerande receptet är att först optimera hela grafen, sedan dela den
vid lagergränserna och slutligen stänga av ytterligare grafoptimering när
delarna laddas. Då bevaras originalets optimerade beräkningar. Tolv delar
delar en enda trådpool med åtta CPU-trådar. Encoder-delarnas separata
minnesarenor är avstängda och gamla mellanresultat frigörs löpande.

| Mätning | Tidigare Rust | Slutlig prototyp |
|---|---:|---:|
| Modelladdning | 131,08 s | 7,84 s |
| Support, 35,59 s ljud | 8,63 s | 7,64 s |
| Diktat, 8,07 s ljud | 7,43 s | 6,14 s |
| Siffror, 9,80 s ljud | 7,38 s | 6,86 s |
| Negationer, 9,60 s ljud | 7,63 s | 6,42 s |

Taltiderna är median av körning två och tre, inklusive ljudförbehandling och
avkodning. Alla fyra klipp gav **exakt samma text, tokenföljd och tokenstarter**
som den ursprungliga Rust-prototypen. Fem sekunder digital tystnad gav tom
text. Numerisk encoderkontroll med samma indata vid 1,0, 41,01 och 71,18
sekunder gav **maximalt absolut fel 0 och relativt L2-fel 0** mellan original
och uppdelad optimerad graf. Även utdataformer och längder var identiska.
Detta ersätter inte testning på naturliga möten eller mot NeMo-originalet.

Högsta uppmätta working set för hela Rust-processen var **1,37 GiB**
(1 467 015 168 byte), inklusive modelladdning, samtliga klipp och kontroller.
Avbrytning före inferens, mellan encoder-delar och under avkodning samt
återanvändning efter avbrytning passerade. Fyra Rust-enhetstester, sju
Python-probtester och fyra kontroller av den separata CSE-transformeringen
passerade. CSE används inte i slutlösningen.

Engångsförberedelsen är separat: den tidigare lokala grafoptimeringen tog
167,4 s och uppdelningen av den färdiga cachen 10,83 s. Rust behöver därefter
ingen Python för inferens. Tiden 7,84 s inkluderar kontrollsummor för
encoder-delarna men exkluderar WAV-inläsning och kontroll av originalfilerna
som benchmarkverktyget gör före laddningen. Modelladdning är uppmätt i en
ny process; operativsystemets filcache tömdes inte. Vid runtime-/modellbyte
eller annan hårdvara måste cachen byggas och verifieras igen. Optimerade
layoutgrafer kräver kompatibel hårdvara enligt
[ONNX Runtime-dokumentationen](https://onnxruntime.ai/docs/performance/model-optimizations/graph-optimizations.html#onlineoffline-mode).

Testad kombination: lokal grafcache från ORT 1.30.0 och Rust-körning med
appens `ort = 2.0.0-rc.12` / ORT 1.24.2 på Ryzen 7 7730U. Kompatibilitet
med andra runtimes, processorer eller GPU-backends är inte verifierad.
Detta är en validerad lokal prototyp, inte ett distribuerat modellpaket.

Rekommenderad reproduktion efter att modeller och testljud förberetts:

```powershell
$python = '.build-tools/pianissimo/.venv/Scripts/python.exe'
& $python model-tools/pianissimo-probe.py --prepare-cache-only --avx2-precision `
    --cache-dir .build-tools/pianissimo/ort-cache `
    --output .build-tools/pianissimo/cache-preparation.json
# Utmatningskatalogen ska vara ny. Originalfilerna skrivs inte över.
& $python model-tools/split-pianissimo.py `
    --input .build-tools/pianissimo/ort-cache/encoder.onnx `
    --optimized-cache-manifest .build-tools/pianissimo/ort-cache/manifest.json `
    --output .build-tools/pianissimo/split-optimized
./model-tools/build-pianissimo-probe.ps1
$exe = 'model-tools/pianissimo-rust-probe/target/release/avskrift-pianissimo-probe.exe'
$env:PIANISSIMO_ENCODER_PARTS = '.build-tools/pianissimo/split-optimized/manifest.json'
$env:PIANISSIMO_SHARED_POOL = '1'
$audio = @('support','dictation','numbers','negations','silence') |
    ForEach-Object { ".build-tools/pianissimo/fixtures/$_.wav" }
& $python model-tools/measure-process.py --output .build-tools/pianissimo/rust-optimized-memory.json `
    -- $exe .build-tools/pianissimo/model .build-tools/pianissimo/rust-optimized.json @audio
Remove-Item Env:PIANISSIMO_ENCODER_PARTS
Remove-Item Env:PIANISSIMO_SHARED_POOL
& $python model-tools/compare-pianissimo-variants.py `
    --reference .build-tools/pianissimo/rust-cpu.json `
    --candidate .build-tools/pianissimo/rust-optimized.json `
    --output .build-tools/pianissimo/rust-optimized-parity.json
& $python model-tools/verify-pianissimo-encoder.py `
    --parts .build-tools/pianissimo/split-optimized/manifest.json `
    --audio .build-tools/pianissimo/fixtures/support.wav `
    --reference-cache .build-tools/pianissimo/encoder-reference `
    --output .build-tools/pianissimo/encoder-optimized-parity.json
```

Rårapporterna och alla modellartefakter ligger under `.build-tools/pianissimo`.
`encoder-optimized-parity.json` jämför tensorer med Python/ORT 1.30.0;
`rust-optimized-parity.json` kräver exakt text/token/tidsparitet mellan
Rust-varianterna, utan tolerans på tidsstämplarna. De tidigare misslyckade
försöken beskrivs nedan för att de inte ska blandas ihop med slutresultatet.

## Uppföljning: mindre grafer och snabbare start

**Den första rådelningen nedan är inte godkänd som slutlösning.** Den klarade
transkripttesterna men inte den senare numeriska encoder-kontrollen på 1,0,
41,01 och 71,18 sekunders indata. Relativt L2-fel var cirka 5,6–8,5 procent.
Textlikhet ensam var alltså otillräcklig som kontroll. Fortsatt validering
gäller en uppdelning av den redan optimerade grafen med ytterligare
grafoptimering avstängd för delarna.

Encodern kan delas vid Conformer-lagrens befintliga gränser. Den testade
varianten har tolv ONNX-filer med två lager per fil. Beräkningsnoder och
vikter behålls; verktyget flyttar bara beroenden till rätt del och bevarar
typerna vid delarnas in- och utgångar. Varje fil har en SHA-256-kontrollsumma
i ett manifest knutet till den fastlåsta originalexporten. Inga ljud skickas
till någon tjänst.

Den första färdiga Rust-körningen laddade på **39,03 s**, mot tidigare
**131,08 s** med en sammanhängande encoder. Den uppdelade encoderns laddning
inkluderar även kontroll av delarnas filhashar. Högsta uppmätta working set
för hela Rust-processen var **1,42 GiB**. Den första Python-varianten med
separata minnespooler nådde 4,41 GiB; Rust-varianten stänger därför av
CPU-arena för encoder-delarna och släpper mellanresultat efter sista användning.
Decodern behåller sin lilla minnespool. Inga specialbyggen av ONNX Runtime krävs.

Alla fyra talfall hade **exakt samma text, tokenföljd och tokenstarttider**
som tidigare Rust-körning med ORT 1.24.2. Tre upprepningar per fall var
identiska. Fem sekunder digital tystnad gav tom text. Avbrytning före inferens,
mellan encoder-delar och under avkodning samt återanvändning efter avbrytning
passerade. Det går nu att avbryta mellan två encoder-delar, men inte mitt i
ett synkront ONNX-anrop.

Den här första inställningen använde separata trådpooler utan spinning.
Varma tider blev 11,21 / 8,97 / 9,20 / 9,26 s för support, diktat, siffror
och negationer. De tidigare Rust-tiderna var 8,63 / 7,43 / 7,38 / 7,63 s.
Kortare start ska alltså inte förväxlas med snabbare transkribering. Det är
enskilda mätsessioner på samma dator, inte en systematiskt randomiserad studie.

Två andra försök gav ingen förbättring: sammanslagning av identiska uttryck
minskade grafen från 67 178 till 29 658 noder men tog 243,46 s att ladda;
borttagning av interna formbeskrivningar avbröts efter cirka 280 s utan
färdig modelladdning. Dessa verktyg finns kvar för reproduktion och ska inte
användas som rekommenderade optimeringar.

Körning (PowerShell 7, från repots rot):

```powershell
$python = '.build-tools/pianissimo/.venv/Scripts/python.exe'
& $python -m pip install -r model-tools/requirements-pianissimo.txt
# Målkatalogen måste vara ny; originalmodellen skrivs inte över.
& $python model-tools/split-pianissimo.py --output .build-tools/pianissimo/split-2
./model-tools/build-pianissimo-probe.ps1
$exe = 'model-tools/pianissimo-rust-probe/target/release/avskrift-pianissimo-probe.exe'
$env:PIANISSIMO_ENCODER_PARTS = '.build-tools/pianissimo/split-2/manifest.json'
$audio = @('support','dictation','numbers','negations','silence') |
    ForEach-Object { ".build-tools/pianissimo/fixtures/$_.wav" }
& $python model-tools/measure-process.py --output .build-tools/pianissimo/rust-split-memory.json `
    -- $exe .build-tools/pianissimo/model .build-tools/pianissimo/rust-split.json @audio
Remove-Item Env:PIANISSIMO_ENCODER_PARTS
& $python model-tools/compare-pianissimo-variants.py `
    --reference .build-tools/pianissimo/rust-cpu.json `
    --candidate .build-tools/pianissimo/rust-split.json `
    --output .build-tools/pianissimo/rust-split-exact-parity.json
& $python model-tools/verify-pianissimo-encoder.py `
    --parts .build-tools/pianissimo/split-2/manifest.json `
    --audio .build-tools/pianissimo/fixtures/support.wav `
    --output .build-tools/pianissimo/encoder-length-parity.json
```

`rust-split.json` innehåller transkript, tider, proveniens och avbrytningskontroller.
`rust-split-memory.json` mäter hela barnprocessen, inklusive modelladdning.
Grundinställningen utan `PIANISSIMO_ENCODER_PARTS` laddar fortfarande den
sammanhängande originalexporten. Appens UI och standardmodell är oförändrade.

## Första jämförelsen

Den första tabellen nedan gäller Python-referensen. Den senare Rust-mätningen
och startprofileringen redovisas i avsnittet **Rust-prototyp och profilering**.

Pianissimo fungerar lokalt med den testade exporten när ONNX Runtimes
AVX2-precisionsläge aktiveras. Standardläget gav tom text på både hela
supportsamtalet och ett tiosekundersutdrag. Att stänga av grafoptimeringar eller
välja basic-optimering löste inte felet. Med samma oförändrade modellvikter och
`session.x64quantprecision=1` återkom korrekt text. Det är ett avgörande
kompatibilitetskrav för denna kombination av export, runtime och processor.

Varm CPU-tid, median av två körningar efter första körningen:

| Testljud | Ljudlängd | Pianissimo INT8, precisionsläge | KB-Whisper small Q5 | KB-Whisper medium Q5 |
|---|---:|---:|---:|---:|
| Supportsamtal | 35,59 s | 8,89 s | 10,00 s | 29,41 s |
| Diktat med namn | 8,07 s | 7,14 s | 4,05 s | 12,68 s |
| Siffror | 9,80 s | 7,93 s | 4,25 s | 12,38 s |
| Negationer | 9,60 s | 7,25 s | 4,28 s | 12,50 s |

Pianissimo hade noll normaliserade ordfel i supportsamtalet, diktatet och
negationsfallet. Whisper small hade 3/79 ordfel i supportsamtalet och medium
1/79; båda hade noll i diktatet och negationerna. I sifferfallet fick alla
8/19 enligt strikt WER. Transkripten återgav dock de avsedda siffrorna med
siffror/förkortningar, medan referensen använder utskrivna ord. Exempel från
Pianissimo: `23`, `14.30` och `1200 kr.`. Sifferfallet är därför främst ett
exempel på varför rå WER behöver kompletteras med innehållsgranskning.

Resultaten var identiska mellan de tre körningarna per testfall. Pianissimos
tokenstarttider var monotona och inom ljudets längd. Tiderna har inte granskats
mot manuellt uppmärkta ordgränser. Högsta rapporterade working set i
Pianissimo-processen var cirka 1,88 GiB, inte ett generellt minneskrav.

Modelladdningen tog **141,6 sekunder** i den fungerande körningen. En separat
kontroll med sparade optimerade ONNX-grafer gav **142,8 sekunder** även efter
att cachen skapats. Grafcachen löser alltså inte starttiden. Kontrollsummor och
WAV-inläsning ligger utanför dessa laddningstider. Cacheflaggan i verktyget
finns för att kunna reproducera kontrollen; den är ingen rekommenderad
prestandaförbättring. Uppföljande profilering redovisas längre ned.
Text, tokenföljd och tokenstarttider var exakt lika med och utan grafcache på
samtliga fyra testfall. Cacheförberedelsen tog ytterligare 167,4 sekunder.

Slutsats för denna CPU: Pianissimo är lovande för filer och bättre än medium
i hastighet på dessa exempel. Den är långsammare än small för korta diktat.
Underlaget motiverar fortsatt integration som ett alternativ, men inget
standardbyte. GPU och verkliga möten återstår.

Sju automatiska kontroller passerar för bland annat svensk textnormalisering,
borttappade negationer, tomma referenser, fel ljudformat och cacheinvalidering.
Whisper-proben är byggd i release-läge och faktiskt körd med båda modellerna.
Ingen fullständig apptestning har gjorts eftersom appens körkod inte ändrats.

Lokala råresultat: `.build-tools/pianissimo/comparison.json` (samlad jämförelse),
`cpu-precision.json` (fungerande Pianissimo-körning), `cpu-cached.json`
(cachekontroll) och `whisper-*-cpu.json` (Whisper-körningar). De innehåller
de fiktiva transkripten, tider och modell-/ljudkontrollsummor. De första
misslyckade ONNX-körningarna finns separat och ingår inte i hastighetstabellen.

## Modell och avgränsning

- Original: [KlangAI/pianissimo-sv](https://huggingface.co/KlangAI/pianissimo-sv),
  revision `8f1f6d8f8bd7482a5ea1d2bfaf6ef5be61597138`.
- Testad community-export: [moonhouse/pianissimo-sv-onnx](https://huggingface.co/moonhouse/pianissimo-sv-onnx),
  revision `72c38267654dadd538bceac7a851de00fb55f11a`.
- Encoder: 904 177 058 byte, SHA-256
  `8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2`.
- Decoder/joint: 18 300 628 byte, SHA-256
  `2fb4ef1c1e28839aef70e74a3a2737afdc7460afa1e640ce1c4f9bf9ceadcb51`.
- Modellkorten anger CC BY 4.0. Originalmodell: Klang AI AB. ONNX-export och
  kvantisering: moonhouse. Vikterna ändras inte av prototypen.

Exporten har en sammanfogad decoder/joint, medan Sherpas vanliga Parakeet-paket
har separata encoder-, decoder- och joiner-filer. Prototypen använder därför
ONNX Runtime genom onnx-asr i Python-referensen och direkt genom ort i
Rust-prototypen, inte Avskrifts befintliga sherpa-rs-bindning.
Python har inte installerats som komponent i skrivbordsappen.

Originalets `model_config.yaml` lästes direkt från NeMo-arkivet. Den anger
16 kHz, 128 melband, 25 ms Hann-fönster, 10 ms steg, FFT 512 och normalisering
per frekvensband. Community-konfigurationens `features` anpassas till
onnx-asr:s `features_size` genom en liten modellklass. Tokenavkodningen använder
TDT med varaktigheterna 0–4 och blank-ID 8192. Originalets NeMo-inferens har
inte körts; ingen numerisk paritet med originalet är därmed verifierad.

## Testmiljö och metod

- Windows, AMD Ryzen 7 7730U, 8 kärnor/16 trådar, cirka 16 GB RAM.
- CPU, åtta beräkningstrådar. Datorns integrerade Radeon-GPU ingår inte i mätningen.
- Python 3.14, onnx-asr 0.12.0, ONNX Runtime 1.30.0 och NumPy 2.5.3.
- Jämförelse: separat release-binär med whisper-rs 0.14.4 och Avskrifts
  avkodningsinställningar: svenska, greedy/best_of=1, ingen tidigare textkontext,
  ingen översättning, åtta trådar och inga ordtidsstämplar.
- KB-Whisper small och medium är användarens redan hämtade Q5-modeller.
- Tre körningar per testfall. Modelladdning mäts separat. Varm tid är medianen
  av körning två och tre. WAV-inläsning ingår inte; ljudförbehandling och
  avkodning ingår. Modellerna körs efter varandra.

Fyra fiktiva inspelningar med Microsoft Bengt: ett supportsamtal från projektet,
ett diktat med namn, ett fall med siffror och ett med negationer. Referensen för
supportsamtalet inkluderar talaretiketterna, eftersom de läses upp i ljudfilen.

WER räknas med gemener och skiljetecken ersatta av mellanrum. Sifferstavning
normaliseras inte: `två` och `2` räknas därför som olika ord. Granska siffrorna
semantiskt också. Dessa få syntetiska klipp säger inget säkert om dialekter,
naturliga möten, överlappande tal eller fackspråk.

## Köra om

Använd PowerShell 7, Python och Visual Studio C++ Build Tools för Whisper-proben.
Välj en kort byggsökväg i en lokal arbetskatalog. Alla modellfiler, råtexter och
mätresultat ska ligga i den ignorerade `.build-tools`-katalogen.

```powershell
python -m venv .build-tools/pianissimo/.venv
$python = '.build-tools/pianissimo/.venv/Scripts/python.exe'
& $python -m pip install -r model-tools/requirements-pianissimo.txt
./model-tools/fetch-pianissimo.ps1
./model-tools/prepare-speech-probe.ps1
& $python model-tools/test-pianissimo-probe.py

$audio = @('support', 'dictation', 'numbers', 'negations') |
    ForEach-Object { ".build-tools/pianissimo/fixtures/$_.wav" }
& $python model-tools/pianissimo-probe.py --audio $audio `
    --output .build-tools/pianissimo/cpu-precision.json --avx2-precision

# Valfri cache av optimerade grafer. Förberedelse mäts separat från modelladdning.
& $python model-tools/pianissimo-probe.py --audio $audio `
    --output .build-tools/pianissimo/cpu-cached.json --avx2-precision `
    --cache-dir .build-tools/pianissimo/ort-cache

# Ange en kort sökväg och dina modellfiler:
$target = 'C:/avskrift-probe'
./model-tools/build-whisper-probe.ps1 -TargetDir $target -Backend CPU `
    -LibClangPath .build-tools/pianissimo/.venv/Lib/site-packages/clang/native
$models = Join-Path $env:APPDATA 'com.avskrift.app/whisper-models'
foreach ($size in @('small', 'medium')) {
    foreach ($case in @('support', 'dictation', 'numbers', 'negations')) {
        & "$target/release/avskrift-whisper-probe.exe" `
            "$models/kb-whisper-$size.bin" `
            ".build-tools/pianissimo/fixtures/$case.wav" `
            ".build-tools/pianissimo/whisper-$size-$case-cpu.json" cpu
    }
}
$whisper = Get-ChildItem .build-tools/pianissimo/whisper-*-cpu.json |
    Select-Object -ExpandProperty FullName
& $python model-tools/compare-speech-probe.py `
    --pianissimo .build-tools/pianissimo/cpu-precision.json `
    --whisper $whisper --output .build-tools/pianissimo/comparison.json
```

Utelämna `--avx2-precision` för ONNX Runtimes standardläge. Flaggan aktiverar
`session.x64quantprecision=1`, ett dokumenterat läge för att undvika U8S8-overflow
på SSE4.1/AVX2/AVX512-processorer utan VNNI. Detta är en separat kontroll,
inte ett antagande att exporten är korrekt. Se
[ONNX Runtimes konfigurationsnycklar](https://github.com/microsoft/onnxruntime/blob/main/include/onnxruntime/core/session/onnxruntime_session_options_config_keys.h)
och [kvantiseringsdokumentationen](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html).

Probeverktyget kräver lokala modellfiler och gör inga nätanrop vid inferens.
Det kontrollerar vikternas SHA-256, WAV-formatet och ändliga tensorvärden.
Tom text för ett testfall med talreferens ger en felstatus; den får aldrig
rapporteras som en lyckad snabb transkribering. Tokenstarttider är inte samma
sak som verifierade ordintervall med start- och sluttid.
Prototypen avvisar ljud längre än 120 sekunder; den implementerar inte
uppdelning och återmontering av långa möten. Avskrifts befintliga Vulkan-flagga
aktiverar Whisper/llama.cpp, inte automatiskt en ny ONNX-motor.

## Villkor före integration och eventuellt standardbyte

1. Verifiera avkodningen mot originalmodellen på samma ljud, inklusive långa
   inspelningar och tystnad. Håll exportfel åtskilda från modellkvalitet.
2. Implementera en lokal Rust-motor med verifierad export, tidsstämplar,
   minnesgränser, avbrytning och modellcache. Pröva CPU och avsedd GPU-väg.
3. Prova naturligt svenskt tal, namn, siffror, negationer och överlappande tal
   samt synkad uppspelning, diarisering och export i appen.
4. Lägg till som valbar modell först när den fungerar. Behåll KB-Whisper och
   dess översättningsläge. Ett standardbyte kräver bättre resultat på den
   aktuella datorn och ska inte skriva över ett uttryckligt modellval.

## Rust-prototyp och profilering

På samma fyra ljud gav Rust exakt samma text och tokenföljd som
Python-referensen. Ljudförbehandlingens största absoluta avvikelse var
`4,98e-5` (kontrollgräns `0,001`). Diktatets tokenstarter var exakt lika;
övriga tre klipp hade enstaka skillnader på högst **80 ms**, ett encodersteg.
Strikt tidsparitet passerar alltså inte. Jämförelsen ändrar både implementation
och runtime-version; orsaken till tidsavvikelserna har inte isolerats.
Verifieringsverktyget kräver exakt matchning som standard och redovisar även
exakt-match-resultatet när man uttryckligen tillåter 80 ms. Detta ersätter
inte kontroll mot manuellt uppmärkta ordgränser.

Åtta CPU-trådar, median av körning två och tre, sekunder:

| Klipp | Rust totalt | Förbehandling | Encoder | Decoder/text |
|---|---:|---:|---:|---:|
| Support | 8,63 | 0,140 | 8,405 | 0,088 |
| Diktat | 7,43 | 0,025 | 7,368 | 0,033 |
| Siffror | 7,38 | 0,030 | 7,327 | 0,026 |
| Negationer | 7,63 | 0,032 | 7,560 | 0,040 |

Rust-modellen laddades på **131,08 s**, varav encoder **130,91 s**.
Det är samma storleksordning som Python; en Rust-port löser inte starttiden.
Rapportens runtime-build är `rel-1.24.2`, commit `058787c`, Release.

En separat Rust-körning med **en tråd** laddades på 95,55 s och gav samma
text/tokenföljd. Varma tider var 12,03 / 8,77 / 8,53 / 8,46 s för support,
diktat, siffror respektive negationer. Alla var långsammare än med åtta trådar.
Fem sekunder digital tystnad gav tom text, utan icke-ändliga tensorer
(11,24 s varm tid). Tystnadskontrollen gäller ett enda rent nollsignaltest,
inte bakgrundsbuller. Den enskilda kortare starten visar inte någon robust
förbättring; Python-kontrollen med en tråd tog 141,62 s. Åtta trådar kvarstår
därför som prototypens standard.

Separat initieringsprofil med Python/ORT 1.30.0:

| Inställning | Encoderstart | Decoderstart |
|---|---:|---:|
| 8 trådar, standard + precisionsläge | 128,93 s | 0,27 s |
| Utan prepacking | 153,31 s | 0,31 s |
| Flush-to-zero | 156,45 s | 0,28 s |
| 1 tråd, detaljerad loggning | 141,62 s | 0,20 s |

Dessa är enskilda diagnostikkörningar, inte upprepade prestandamätningar.
Ingen av ändringarna eliminerade flaskhalsen. I standardprofilen lästes filen
sekventiellt på 1,03 s; ORT:s `model_loading_uri` tog 2,02 s och
`session_initialization` 126,90 s. Det är alltså inte filöverföringen som
förklarar väntan. Den detaljerade loggen visar **24 462 encoder-noder** efter
grafoptimering och ett långt logguppehåll efter att ORT går in i
allokeringsplaneringen (`CreateGraphPartitioner`). Detta pekar på planeringen
av den stora exportgrafen som fortsatt felsökningsområde, men är inte en
funktionsprofil eller ett bevis för en viss intern algoritm. Kortare laddning
kräver mer arbete med export/graf/runtime, inte fler obevisade cacheflaggor.

Rårapporter: `rust-cpu.json`, `rust-parity.json` (strikt kontroll, ej godkänd
för tidsstämplar), `rust-parity-one-frame.json` (80 ms uttrycklig tolerans),
`profile-default.json`, `profile-no-prepacking.json`, `profile-flush.json`
och `profile-one-thread.json`, samtliga under `.build-tools/pianissimo`.
En-trådskörningen inklusive tystnad ligger i `rust-one-thread.json`.
Dess text-/tokenkontroll mot Python passerade också med högst 80 ms skillnad
i tokenstarter (`rust-one-thread-parity.json`). Återanvändning efter andra
inspelningar, avbrytning före inferens och avbrytning under avkodning med
efterföljande återanvändning passerade. Fyra Rust-enhetstester och de sju
Python-testerna passerade. Full app-, GPU- och NeMo-originaltestning återstår.

`model-tools/pianissimo-rust-probe` är en separat körbar CPU-prototyp med
`ort = 2.0.0-rc.12`, samma pinning som Avskrift. Standardbygget hämtar ONNX
Runtime 1.24.2. Ingen Python behövs vid inferens. Koden har egen NeMo-kompatibel
ljudförbehandling (RustFFT, Slaney-filterbank, normalisering) och greedy
TDT-avkodning. Python/onnx-asr 0.12.0 används som oberoende jämförelse.

Prototypen verifierar SHA-256 för båda modellgraferna, konfigurationen och
ordlistan. Den återanvänder modellsessionerna och återställer avkodartillståndet
för varje inspelning. Återkörning av första filen efter de andra kontrollerar
att inget tillstånd läcker. Avbrytning kontrolleras före förbehandlingen,
före encodern och mellan avkodningsstegen. Pågående synkrona ONNX-anrop kan
ännu inte avbrytas. Modellen är inte kopplad till appens UI, arbetskö,
modellcache, diarisering eller filnedladdning.

Kör från repots rot i PowerShell 7:

```powershell
./model-tools/build-pianissimo-probe.ps1
./model-tools/build-pianissimo-probe.ps1 -Action test
$exe = 'model-tools/pianissimo-rust-probe/target/release/avskrift-pianissimo-probe.exe'
$audio = @('support', 'dictation', 'numbers', 'negations') |
    ForEach-Object { ".build-tools/pianissimo/fixtures/$_.wav" }
& $exe .build-tools/pianissimo/model .build-tools/pianissimo/rust-cpu.json @audio
$python = '.build-tools/pianissimo/.venv/Scripts/python.exe'
& $python model-tools/verify-pianissimo-rust.py --exe $exe `
    --python-report .build-tools/pianissimo/cpu-precision.json `
    --rust-report .build-tools/pianissimo/rust-cpu.json `
    --timestamp-tolerance 0.08 `
    --output .build-tools/pianissimo/rust-parity.json
& $python model-tools/profile-pianissimo.py `
    --output .build-tools/pianissimo/profile-default.json

# Separat en-trådskontroll inklusive fem sekunder digital tystnad:
& $python -c "import wave; w=wave.open('.build-tools/pianissimo/fixtures/silence.wav','wb'); w.setparams((1,2,16000,0,'NONE','not compressed')); w.writeframes(bytes(160000)); w.close()"
$env:PIANISSIMO_THREADS = '1'
& $exe .build-tools/pianissimo/model .build-tools/pianissimo/rust-one-thread.json `
    @audio .build-tools/pianissimo/fixtures/silence.wav
Remove-Item Env:PIANISSIMO_THREADS
```

Rust-probens miljövariabler `PIANISSIMO_THREADS`, `PIANISSIMO_FLUSH=1` och
`PIANISSIMO_NO_PREPACK=1` styr separata experiment. Python-profileraren har
motsvarande `--threads`, `--flush-to-zero`, `--no-prepacking` samt `--no-spin`
och `--verbose`. Ändra en faktor i taget och kör inte flera CPU-mätningar
samtidigt. Laddningstiderna omfattar sessionsskapandet, inte SHA-kontrollen.
