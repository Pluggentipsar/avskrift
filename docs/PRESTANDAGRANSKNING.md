# Prestandagranskning av AVskrift – 2026-09-11

Genomgång av taltranskribering, diktering, mötesinspelning, ljudomvandling, ekoborttagning,
diarisering, avidentifiering, språkmodeller, historik, gränssnitt och export. Målet är snabbare
bearbetning och lägre minnestryck både med GPU och på kontorsdatorer. Den första uppsättningen
ändringar är genomförd; återstående större kandidater är separerade nedan.

## Genomförda ändringar, i prioritetsordning

| Prioritet / område | Tidigare beteende | Ändring och praktisk effekt |
|---|---|---|
| P1: möteskö (`capture.rs`, `lib.rs`) | Obegränsad kö av ljud om direktranskriberingen inte hinner med. | Högst åtta väntande ljudblock, cirka 17 MB vid 48 kHz och max 11 sekunder/block. Full kö blockerar aldrig inspelningen. Överbelastningen markeras varaktigt, och slutresultatet byggs från de fullständiga WAV-filerna även om arbetstråden senare hunnit klart. |
| P1: mötesbearbetning (`lib.rs`) | Ljudet lästes och ekobearbetades igen för uppspelningsmixen efter samlad transkribering. | Samma avkodade och ekorensade ljud används till både text och mix. Mixen skrivs direkt till fil utan ytterligare en hel ljudbuffert. En timme mono float32/16 kHz motsvarar cirka 220 MiB per sådan buffert. |
| P2: AI-minne (`llm.rs`) | Varje fråga reserverade 8192 positioner och ett stort inläsningsbatch, även för kort text. | Arbetsfönstret dimensioneras efter verkligt tokenantal plus hela svarskvoten, avrundat till 256. Inläsning sker i block om 512 token med bibehållna positioner. För lång indata ger ett läsbart fel före native-anropet. Ingen tyst avklippning av underlaget. |
| P2: modellbyte och AI-trådar (`lib.rs`, `llm.rs`) | Ny sammanfattningsmodell laddades innan den gamla frigjordes. Alla logiska CPU-trådar användes. | Den gamla modellen frigörs först, vilket minskar RAM/VRAM-toppen. Högst åtta fysiska CPU-kärnor används. Sista genererade token skickas inte igenom en onödig extra decode. |
| P2: NER (`pii/model.rs`) | ONNX valde automatiskt trådar och väntade aktivt mellan arbetspass. | Högst fyra CPU-trådar och ingen aktiv spinning. Den lokala jämförelsen gav samma kategorier och textintervall. |
| P2: eko (`aec.rs`) | Varje möjlig fördröjning beräknades separat över samma ljudfönster. | Linjär korskorrelation beräknas med FFT och nollutfyllnad. Samma sökområde och energikontroll behålls. Helt tyst referens ger direkt oförändrad mikrofon. Flyttalsavrundning kan skilja vid nästan lika korrelationstoppar. |
| P2: långa avidentifieringar (`engine.rs`, `pii/merge.rs`) | Stycken och redan accepterade träffar söktes igenom linjärt för varje nytt segment/träff. | Binär sökning för stycken och ett sorterat träd för överlappskontroll. Prioritet, resultatordning och maskeringsregler är oförändrade. Överlappshanteringen går från kvadratisk till O(n log n). |
| P2: historik (`lib.rs`, `+page.svelte`) | Diskgenomgång startade för varje tangenttryckning; kommandona arbetade synkront på UI-tråden. Gamla söksvar kunde ersätta nya. | Sökning samlas efter 250 ms, gamla svar ignoreras och historik/åtaganden läses på bakgrundstrådar. |
| P3: ljudomvandling (`audio.rs`) | Varje 1024-samplersblock kopierades och nya utbuffertar skapades. | Indata lånas och utbufferten återanvänds. Jämförelsetestet gav exakt samma ljudsampler för tre samplingsfrekvenser och hela/delvisa block. |

Whisper-optimeringarna från föregående steg – optimerad native-kompilering, GPU-stöd,
återanvänt modellminne och förberedelse av dikteringsmodellen – är kvar. Se [PRESTANDA.md](PRESTANDA.md).

## Mätningar och verifiering

Maskin: Intel Core Ultra 7 265KF och RTX 5070 Ti 16 GB. Syntetisk text och syntetiskt ljud;
inga användarinspelningar eller privata transkript användes. Detta är inte en mätning på en
kommunal standarddator och inte ett löfte om motsvarande vinst på annan hårdvara.

| Mätning | Före / referens | Efter |
|---|---:|---:|
| NER, 20 upprepade syntetiska meningar, varm modell, två körningar | 122 / 85 ms | 44 / 44 ms |
| Fördröjningssökning för eko, 32000-samplers fönster, ±8000 samplers | 204 ms | 6,5 ms |

NER gav samma 60 träffar, inklusive identiska start/slutpositioner och kategorier, i alla sex
körningar. Ekomätningen gav samma fördröjning (200 sampler). Den mäter bara fördröjningssökningen;
det adaptiva ekofiltret och övriga mötessteg ingår inte.

Ett riktigt lokalt Qwen-GGUF testades med en syntetisk faktafråga på 1757 indatatoken, en annan
fråga och sedan den första igen. Modellen svarade på faktafrågan och gav samma första/sista svar.
Native-loggen bekräftade GPU-avlastning av 29/29 lager. Arbetsfönstret blev 1792 positioner med
49 MiB KV-cache; den kortare frågan använde 512 positioner och 14 MiB. Modellvikter och
beräkningsbuffertar tillkommer. Första körningen inkluderade GPU-förberedelser och ska inte
användas som ett mått på normal svarstid. Testet ersätter inte en bred kvalitetsutvärdering.

Samma integrationstest passerade även med CPU-bygget: loggen visade 0/29 avlastade lager
och KV-cache på CPU. CPU-bygget stänger uttryckligen av både lager- och operationsavlastning,
så ett GPU-kapabelt bibliotek i testsökvägen inte ändrar valet. Det separata CPU-paketets
`ggml.dll` kontrollerades också och har inget beroende på `ggml-vulkan.dll`.

68 ordinarie Rust-tester passerade. Frontendkontrollen gav noll fel och tre redan befintliga
varningar (två dialogers fokusattribut och saknade Node-typdefinitioner).

Ordinarie regressionstester omfattar bland annat mättad möteskö, eko i båda riktningar och vid
fönsterkanter, ljudomvandling, mixens klippning/nollutfyllnad, AI:s tokenbudget, styckesgränser
och jämförelse med den tidigare överlappsalgoritmen på 3000 syntetiska träffar.

Kör vanliga tester med `cargo test --release --features vulkan --lib`. Extra lokala tester:

- `benchmark_ner_threads -- --ignored --nocapture`
- `benchmark_echo_delay -- --ignored --nocapture`
- `real_model_handles_batched_prompts_and_independent_requests -- --ignored --nocapture`
  med `AVSKRIFT_BENCH_LLM` satt till en befintlig lokal GGUF.

Använd byggmiljön och den korta `--target-dir` som beskrivs i [PRESTANDA.md](PRESTANDA.md).

Paketen för denna genomgång ligger i `dist/Avskrift-Vulkan-prestanda` och
`dist/Avskrift-CPU-prestanda`. Byggskriptets valfria `-PackageSuffix prestanda` gör att den
tidigare körbara versionen kan ligga kvar. Hela paketmappen behövs. Den nya versionen har
inte startats parallellt med användarens pågående dikteringssession.

## Större kandidater som återstår

1. **Långa ljudfiler:** avkodaren håller fortfarande hela ljudet i källfrekvens före omsampling.
   Strömmande avkodning/omsampling är nästa större RAM-vinst för långa möten. Kräver tester av
   tidsstämplar, samplingsfördröjning och ord vid blockgränser.
2. **Diarisering:** installerade `sherpa-rs` 0.6.8 hårdkodar en tråd per ONNX-delmodell och dess
   publika konfiguration exponerar inget trådantal. Vulkan för Whisper/llama innebär inte GPU-stöd
   här. Separat backend- och kvalitetsprov behövs innan detta ändras.
3. **Automatisk minnesbudget:** GPU-lagren avlastas fortfarande utan ett eget test av ledigt VRAM.
   Gemensam modellcache för AI-granskning/sammanfattning, automatisk avlastning av inaktiva modeller
   och testad CPU-reservväg vid minnesbrist kan hjälpa datorer med små eller integrerade GPU:er.
4. **Mycket långa transkript i UI:** alla ordknappar renderas fortfarande, och uppspelning uppdaterar
   deras markeringar. Rendering av enbart synliga avsnitt behöver provas med sökning, redigering,
   tangentbordsnavigering och automatisk scrollning innan det införs.
5. **Stor historik:** varje sökning läser fortfarande jobbens JSON-filer. Ett återskapningsbart
   metadata-/sökindex skulle hjälpa tusentals jobb; det måste uppdateras korrekt vid alla sparningar,
   flyttar och borttagningar.
6. **Långa AI-underlag:** sammanfattning delas efter ungefärlig textlängd, medan motorn har en
   verklig tokengräns. Tokenbaserad delning och flerstegs sammanfogning behövs för extrema underlag
   och långa egna mallar. Den nya gränskontrollen ger ett fel i stället för att riskera native-krasch.

## Övriga granskningsdimensioner

- **Prestanda:** konkreta CPU-, minnes- och GPU-förbättringar genomförda och avgränsat verifierade.
- **Korrekthet:** relevanta resultatjämförelser tillagda. Ljudinspelning med verkligt överbelastad
  kontorsdator, UI-interaktion och bred modellkvalitet behöver fortfarande manuell validering.
- **Säkerhet:** denna genomgång är en prestandagranskning, inte en fullständig säkerhetsrevision.
  Bearbetningen fortsätter lokalt och testerna använder syntetiska underlag.
- **Underhållbarhet:** jämförelsetester och reproducerbara paket finns. Den stora `+page.svelte`
  och kommandosamlingen i `lib.rs` gör större ändringar mer svåröverskådliga.

Bra befintliga egenskaper: modeller laddas vid behov, ordlistor och regexar återanvänds,
inspelning skriver källjud löpande till disk, nedladdning sker via temporär fil, och CPU-paketet
kan distribueras separat från GPU-paketet. Dessa egenskaper är bevarade.
