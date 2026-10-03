# Avskrift 0.8.0-beta.3 – möten direkt efter stopp och uppdateringar i appen

Förhandsrelease. Allt från 0.8.0-beta.2 (Pianissimo från Klangs egen export, Textklipp, ny
arbetsyta) ingår. Version 0.6.0 ligger kvar som senaste stabila release.

## Nytt

- **Mötet går att läsa direkt efter stopp.** Texten som transkriberades under mötet visas i
  Transkript så fort du stoppar, med besked om vad som återstår. Den sparas i mötet och finns
  kvar om appen stängs. Redigering öppnas när det färdiga transkriptet har kommit.
- **Bara det som saknas transkriberas efteråt.** Om realtidstranskriberingen inte hann med
  (vanligt på CPU) transkriberas bara de delar den missade i stället för hela mötet. Hann den
  med blir det ingen ny körning alls. Vill du ha bästa kvalitet kan du som tidigare välja
  **Transkribera om** med en större modell när mötet är klart.
- **Uppdateringar i appen.** Välj **Sök efter uppdatering** i menyns statusruta. Appen söker
  bara när du ber om det. Finns en ny version laddas den ner, kontrolleras mot Avskrifts
  signatur och installeras med ett klick; appen startar om och ditt arbete påverkas inte. Välj
  mellan stabila versioner och även förhandsversioner. Gäller den installerade versionen; den
  portabla ZIP-versionen visar en länk till releasen i stället.

## Hämta och installera

Det här är den sista versionen som behöver installeras för hand. Installera med
installationsprogrammet så kommer nästa version via **Sök efter uppdatering**.

- **Avskrift_0.8.0-beta.3_x64-setup-vulkan.exe:** installationsprogram, GPU-version för dator
  med kompatibelt grafikkort.
- **Avskrift_0.8.0-beta.3_x64-setup-cpu.exe:** installationsprogram för dator utan Vulkan-stöd.
- **Windows-Vulkan.zip / Windows-CPU.zip:** portabla mappar utan installation (uppdateras inte
  i appen).

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar. Dina arbeten ligger kvar
i appens datamapp. Installationsprogrammen är inte kodsignerade, så Windows SmartScreen kan
varna. Välj *Mer information* och *Kör ändå*.

## Verifierat och återstående

- 115 ordinarie Rust-tester passerade på vardera CPU och Vulkan, bland dem nya tester för vilka
  delar av ett möte som räknas som saknade och att kompletterad text hamnar på rätt tid.
- UI-tester (mockade data) för det preliminära mötestranskriptet, uppdateringsdialogen,
  arbetsytan, Transkribera och grundflödet passerade. Svelte/TypeScript: 0 fel.
- Båda installationsprogrammen är signerade, och signaturerna kontrollerades mot den publika
  nyckeln i appen innan publicering.
- Varje exe- och DLL-fil kontrollerades mot sina beroenden; inga saknades. CPU-versionen har
  inget Vulkan-beroende.
- Inte provat: ett riktigt digitalt möte med den nya kompletteringen, och en uppdatering i appen
  från början till slut. Den första riktiga uppdateringen blir från den här versionen till nästa.
- Känt fel, inte nytt i denna version: talarseparationen kan i sällsynta fall krascha appen på
  långa inspelningar. Det utreds separat.

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
