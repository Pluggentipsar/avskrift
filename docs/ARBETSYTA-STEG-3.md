# Arbetsyta 3 – källutkast, diktatbearbetning och granskning

2026-09-11. Bygger vidare på [arbetsyta 2](ARBETSYTA-STEG-2.md). Möten, diktering och avidentifiering får varsin sammanhängande förbättring.

## Möten: från underlag till utkast

Öppna ett transkript och välj **Sammanfattning → Skapa källutkast**. Standardmallarna är mötesprotokoll, beslut/åtgärder och kort lägesbild. Egna mallar kan sparas på datorn.

Modellen lämnar strukturerade punkter med typ, text, käll-id och ordagrant citat. Svarsformen begränsas med llama.cpp-grammatik. Ett separat kontrollsteg avvisar citat som inte finns exakt i angiven källa. Långa underlag bearbetas i delar utan att citathänvisningar ersätts av AI-genererade tidsstämplar. Ofullständiga eller oläsbara svar ersätter inte tidigare utkast.

**Visa källa** öppnar citat och sammanhang. **Öppna och lyssna** använder tidspositionen från transkriptet; utan tillgänglig ljudreferens visas **Öppna transkriptet**. Källans text och tidsposition sparas tillsammans med förslagen. Ändrad text, talarnamn eller ljudreferens markerar det tidigare källutkastet som inaktuellt och stoppar användning av dess punkter. De tidigare citaten går fortfarande att läsa.

Ett hittat citat är ingen automatisk bekräftelse på att hela påståendet är korrekt. Användaren granskar betydelsen. Punkter kan redigeras; redigering tar bort godkännandet. En åtgärd måste godkännas innan den läggs till i arbetsytans åtgärdslista. Den kan bara läggas till en gång från samma förslag. Ansvarig och datum fylls inte automatiskt i. Godkända punkter kan användas som ett vanligt redigerbart utkast; den tidigare arbetskopian arkiveras först.

Det tidigare fria sammanfattningsflödet finns kvar och är uttryckligen märkt som utkast utan citathänvisningar. Det går inte att garantera att modellen fångar alla relevanta beslut eller åtgärder; hela underlaget behöver fortfarande kontrolleras.

Källutkast utgår från det rättade transkriptet med dess talarnamn. De märks inte som avidentifierade. Vid kopiering för extern AI följer märkningen hur just utkastet skapades; byte av en inställning ändrar inte retroaktivt textens ursprung. Manuell redigering tar bort märkningen som avidentifierad. Äldre utkast utan sparad ursprungsinformation behandlas som originaltext i kopieringsdialogen.

## Diktering: jämför före användning

Välj **Bearbeta text** vid ett diktat. Välj mejl, tjänsteanteckning, rättning/förtydligande eller en sparad egen mall. Underlaget och det redigerbara förslaget visas bredvid varandra. Ingenting ersätts förrän användaren har markerat att texten jämförts och klickat **Godkänn och använd texten**.

Första originalet behålls även efter bearbetning. Om diktatet har ändrats medan förslaget varit öppet avvisas ersättningen. Diktatets befintliga val för sparande gäller: sessionsdiktat stannar i sessionen, sparade diktat och deras original sparas lokalt. Ett ej använt AI-förslag kastas när dialogen stängs. Inget skickas till andra program från bearbetningsdialogen.

Bearbetning använder den valda sammanfattningsmodellen. Om den saknas visas ett fel med instruktion att hämta den under Sammanfatta text. Underlag begränsas till 6 000 tecken per bearbetning för att hålla resursbehovet rimligt.

## Avidentifiering: original och resultat tillsammans

Granskningsvyn visar **Original med markeringar** och **Maskerad text** bredvid varandra när fönstret är tillräckligt brett, annars efter varandra. Förhandsvisningen hämtas från samma maskeringsmotor som exporten. Föråldrade svar från en tidigare förhandsvisning får inte ersätta ett senare resultat.

**Markera texten som granskad** sparar ett användarbeslut för exakt den aktuella texten och de aktuella maskeringarna. Ändrade val behöver granskas igen. Det är inte ett automatiskt godkännande av att alla personuppgifter hittats.

Egna granskningsprofiler sparar kategorier, ordlista och valet av djupare AI-granskning. **Använd och granska igen** tillämpar profilen och kör en ny granskning. Mallar och profiler sparas i appens lokala inställningar; de innehåller inte automatiskt projektens texter.

## Teknik och verifiering

Nya separata komponenter: `GroundedDraft`, `TemplatePicker`, `RewriteDialog`, `ReviewComparison` och `ReviewProfiles`. Arbetskopian lagrar källutkast och granskningsgodkännande genom formatets stöd för utökade fält. Befintlig versionshantering omfattar dessa data. Diktathistoriken har ett bakåtkompatibelt originalfält.

Native-regressioner täcker svenska citat, felaktiga käll-id:n, saknade citat, oläsbara modellsvar, Unicode-säker uppdelning och att sessionsoriginal inte hamnar i den sparade historiken. Det opt-in-test som använder verklig Qwen 2.5 3B kör endast syntetisk text och kontrollerar både källutkast och diktatbearbetning.

Verifierat: 74 vanliga Rust-tester passerar (12 opt-in-tester utelämnas i standardkörningen), två sparningstester och samtliga tre gränssnittssviter passerar. Det nya modelltestet har dessutom körts separat på både Vulkan och CPU, inklusive kontroll att ett avklippt svar ger fel i stället för att användas. Den befintliga modellregressionen för långa promptar och oberoende anrop passerar också. `npm run check` ger noll fel och tre tidigare befintliga varningar. CPU-testets logg visar 0 av 37 lager på GPU och CPU-lagrad modell och KV-cache.

`model-tools/ui-step3.cjs` kontrollerar granskning och tillägg av åtgärder, godkännande efter redigering, återöppning, inaktuellt underlag, diktatgodkännande/original, egna mallar/profiler och maskeringsjämförelse. De tidigare gränssnittstesterna och sparningstesterna finns kvar. Skärmbilder finns i `docs/ui-step3`.

En felaktig dubbelregistrering av genererade token har rättats i den gemensamma modellmotorn: den använda llama.cpp-versionens `sample()` registrerar redan valt token. Vanlig textgenerering och strukturerade svar använder nu samma korrekta grundflöde.

## Paket

- GPU: `dist/Avskrift-Vulkan-kallutkast/avskrift.exe`
- CPU: `dist/Avskrift-CPU-kallutkast/avskrift.exe`

Stäng tidigare AVskrift och starta det nya paketets programfil. Behåll hela paketmappen. Ingen ominstallation behövs, och befintliga modeller och projekt används fortsatt. Sidomenyn visar **arbetsyta 3**.

Fortsatt arbete: bredare kvalitetsutvärdering på verkliga möten, återankring av citat efter större redigeringar, fler dokumentkällor per arbete och export med klickbara källreferenser. Den här versionen sparar tidigare källversioner och markerar dem tydligt; den flyttar inte automatiskt ett gammalt citat till en ny formulering.
