# Avskrift 0.8.0-beta.4 – modellhämtning på fler nätverk och sammanfattningar från extern AI

Förhandsrelease och den första som kan installeras med **Sök efter uppdatering** i appen (från
0.8.0-beta.3). Allt från tidigare förhandsversioner ingår. Version 0.6.0 ligger kvar som senaste
stabila release.

## Nytt

- **Modeller går att hämta på fler nätverk.** Hämtningen använder nu Windows certifikat och
  proxyinställningar, som webbläsaren. Den fungerar därför även där ett antivirusprogram eller en
  brandvägg granskar krypterad trafik, eller där nätverket kräver proxy. Avbrutna hämtningar
  fortsätter där de slutade, och felmeddelandena säger vad som hänt och vad du kan göra.
- **Hämta i webbläsaren.** Går det ändå inte att hämta en modell i appen: under Modeller på datorn
  finns länkar att öppna i webbläsaren. Välj sedan de hämtade filerna, så lägger Avskrift dem på
  rätt plats och kontrollerar dem.
- **Sammanfattningar från en extern AI.** Klistra in från AI i Sammanfattning tar in svaret med
  rubriker, listor och tabeller i behåll, även när du kopierat formaterad text från en chatt i
  webbläsaren. Finns ett utkast väljer du att ersätta det eller lägga till sist.
- **Visa formaterat.** Växla mellan att redigera sammanfattningen och att se den formaterad, med
  tabeller som tabeller. Kopiera ger formaterad text i Word och markdown i andra program.
- **Riktiga Word-dokument.** Export av sammanfattning, mötesunderlag och anteckningar till Word
  ger rubriker, punkt- och numrerade listor och tabeller i stället för markdown-tecken.

## Hämta och installera

**Har du 0.8.0-beta.3 installerad:** välj **Sök efter uppdatering** i menyns statusruta och
**Ladda ner och installera**.

Annars:

- **Avskrift_0.8.0-beta.4_x64-setup-vulkan.exe:** installationsprogram för dator med
  kompatibelt grafikkort.
- **Avskrift_0.8.0-beta.4_x64-setup-cpu.exe:** installationsprogram för dator utan Vulkan-stöd.
- **Windows-Vulkan.zip / Windows-CPU.zip:** portabla mappar utan installation (uppdateras inte i
  appen).

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar för hand. Dina arbeten
ligger kvar i appens datamapp. Installationsprogrammen är inte kodsignerade, så Windows
SmartScreen kan varna. Välj *Mer information* och *Kör ändå*.

## Verifierat och återstående

- Rust-tester på vardera CPU och Vulkan, bland dem nya för felmeddelanden, matchning av hämtade
  filer och Word-exporten. Ett nätverkstest hämtade och fortsatte en avbruten hämtning mot
  Hugging Face, även en riktig modellfil, med identiskt resultat.
- Word-exporten kontrollerades genom att låta Word rendera ett exempeldokument.
- UI-tester (mockade data) för hämtning i webbläsaren, sammanfattning som markdown,
  uppdateringsdialogen, möten, arbetsytan och grundflödet. Svelte/TypeScript: 0 fel.
- Båda installationsprogrammen är signerade och signaturerna kontrollerade mot nyckeln i appen.
- Inte provat: hämtning på ett nätverk med proxy eller granskning av krypterad trafik.
- Känt fel, inte nytt: talarseparationen kan i sällsynta fall krascha appen på långa
  inspelningar. Det utreds separat.

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
