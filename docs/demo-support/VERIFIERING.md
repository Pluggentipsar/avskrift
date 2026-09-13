# Verifiering av mallflödet, 13 september 2026

## Bedömning

Gränssnitt, lagring, mallhantering och manuell AI-överlämning är provade enligt nedan. **Den lokala modellkvaliteten är inte godkänd för en oövervakad eller förutsatt lyckad live-demo.** Använd det tydligt förberedda demoarbetet för ett förutsägbart exempel. En extern AI-tjänsts kvalitet är inte provad här.

## Programtester

- Svelte/TypeScript: 0 fel. Tre tidigare varningar kvarstår, i äldre dialoger och Node-typdefinitioner.
- Ordinarie Rust-svit: 104 tester passerar på både CPU och Vulkan. Täcker även mallvalidering, revisionskonflikt, bevarad mallkopia, atomisk lagring, återöppnade manuella värden och sökning i mallutkast. 22 opt-in-tester ingår inte i detta antal.
- `model-tools/ui-templates.cjs`: syntetisk webbläsare med mockad Tauri-IPC. Provar demoöppning, manuell ändring, navigering/återöppning, modellfel utan förlust av tidigare utkast, nya utkast i stället för överskrivning, mallredigering/revision, import/export, förhandsvisat kopieringspaket, sparat återklistrat svar, exportinnehåll och ändrat underlag. Kontrollerar också att transkriptet förblir oförändrat vid mallbearbetning.
- `model-tools/ui-step9.cjs`: befintligt mötesflöde passerar med mallfunktionen inkopplad.
- Skärmbilder i `docs/ui-templates/` använder uteslutande syntetiska data. Webbläsartestet provar gränssnitt och IPC-kontrakt, inte verkliga Windows-dialoger eller en extern AI-tjänst.

## Verkliga motorer och ljud

`fiktivt-supportsamtal.wav` är skapad lokalt med Windows-rösten Microsoft Bengt. Den lästes via AVskrifts riktiga ljudavkodare och transkriberades med KB-Whisper-small på RTX 5070 Ti/Vulkan. Ljudet innehåller inga verkliga personuppgifter.

Den faktiska transkriberingen bevarade bland annat `plan 2`, `Det hjälpte inte` och supportens plan att undersöka anslutningen. Ett mellanrum tappades i `starta om skrivaren`, som blev `starta omskrivaren`. Detta behöver rättas innan mallbearbetning.

Den slutliga lokala mallvägen kördes med installerad **Qwen2.5-3B-Instruct Q8_0** både på det förberedda textunderlaget och på det verkliga Whisper-resultatet. Alla åtta fält kunde produceras, men kvalitetskraven föll: exempelvis markerades känd plats eller känt problem som saknade och nästa åtgärd återgavs bristfälligt. Det separata testet av motstridiga våningsuppgifter föll också.

Även **Qwen2.5-7B-Instruct Q8_0** provades med samma förberedda text och konfliktfall på Vulkan; det löste inte kvalitetsbristerna. Modellen hämtades till en separat lokal testkatalog och ersatte inte användarens modeller. En enkel kontroll av den befintliga frågefunktionen gav också ett felaktigt svar. Därför är orsaken inte avgränsad till mallfältens utformning; vidare felsökning av modell/motor/prompt behövs.

Tidiga försök använde en äldre 3B Q4_K_M-fil i utvecklingskatalogen, med dåliga resultat på både CPU och GPU. De får inte blandas ihop med testet av användarens installerade Q8-modell. Tidigare promptvarianter och deras resultat räknas inte som godkännande av den slutliga versionen.

Opt-in-testerna behåller kvalitetsassertionerna. De ska fortsätta ge rött när plats, negation, nästa åtgärd eller motstridiga uppgifter tappas; ett fullständigt JSON-/fältformat är inte samma sak som rätt innehåll.

## Återstår

- Få de två lokala kvalitetstesterna godkända med en dokumenterad modell- och motorkonfiguration. Därefter prova fler varierade samtal; två exempel räcker inte för ett generellt kvalitetslöfte.
- Full manuell provning av det nya Windows-paketet: filväljare, urklipp, ljuduppspelning från källhänvisning och Word-exportens utseende.
- Manuell genomgång i kommunens faktiska, godkända AI-miljö och återklistring av dess svar. Ingen Copilot-integration har byggts eller testats.
- Fysisk inspelning med olika mikrofoner/talare. Den syntetiska ljudfilen provar inte detta.
- Mallmedveten bearbetning av mycket långa samtal. MVP:t stoppar tydligt vid sin storleks-/tokengräns.

## Köra om

`npm run check` och `model-tools/ui-templates.cjs` används för frontend. Webbläsartestet behöver en Vite-server på port 1420 och Playwright/Edge.

För modeller används de ignorerade Rust-testerna `templates::tests::template_demo_with_real_models` och `templates::tests::template_conflict_and_instructions_with_real_model`. Sätt:

- `AVSKRIFT_DRAFT_MODEL`: faktisk GGUF-fil; kontrollera loggens modellnamn och kvantisering.
- `AVSKRIFT_BENCH_MODEL`: lokal KB-Whisper-small-fil.
- `AVSKRIFT_TEMPLATE_AUDIO`: sökvägen till den fiktiva WAV-filen.

Kör med `--ignored --nocapture --test-threads=1` och samma Windows-byggmiljö som appen. Vanliga Rust-tester körs utan `--ignored`. Lokala råloggar finns i den ignorerade `.build-tools`-katalogen; de använder endast syntetiskt innehåll.
