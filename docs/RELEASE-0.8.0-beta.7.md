# Avskrift 0.8.0-beta.7 – skilj på deltagarna i möten

Förhandsrelease. Kan installeras med **Sök efter uppdatering** i appen från 0.8.0-beta.3 eller
senare. Allt från tidigare förhandsversioner ingår. Version 0.6.0 ligger kvar som senaste stabila
release.

## Nytt

- **Skilj på deltagarna.** I ett digitalt möte hörs alla andra än du i samma ljudspår. Nu delas det
  upp efter röst i Talare 1, Talare 2 … när mötet är klart. Inför mötet anger du hur många övriga som
  deltar, eller låter antalet komma från deltagarlistan. Texten blir klar först och rösterna delas
  upp i bakgrunden, så du behöver inte vänta.
- **Ge talarna namn.** Ovanför transkriptet finns en ny panel, Talare. Lyssna på ett kort stycke
  av varje talare och välj namnet ur deltagarlistan. Namnet ändras i hela transkriptet.
- **Slå ihop två talare** som egentligen är samma person. Det går att ångra.
- **Skilj på deltagarna igen** med ett annat antal, från panelen eller knappen i mötesöversikten.
  Det går att pausa och avbryta som andra arbeten.
- Uppdelningen efter röst går ungefär tre gånger snabbare än förut.

Din egen röst heter som förut ”Jag”. Röster som liknar varandra, mycket prat i munnen på varandra
och dålig ljudkvalitet kan fortfarande ge fel. Rätt antal deltagare och sammanslagning gör det
lättare att rätta.

## Hämta och installera

**Har du 0.8.0-beta.3 eller senare installerad:** välj **Sök efter uppdatering** i menyns
statusruta och **Ladda ner och installera**.

Annars:

- **Avskrift_0.8.0-beta.7_x64-setup-vulkan.exe:** installationsprogram för dator med
  kompatibelt grafikkort.
- **Avskrift_0.8.0-beta.7_x64-setup-cpu.exe:** installationsprogram för dator utan Vulkan-stöd.
- **Windows-Vulkan.zip / Windows-CPU.zip:** portabla mappar utan installation (uppdateras inte i
  appen).

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar för hand. Dina arbeten
ligger kvar i appens datamapp. Installationsprogrammen är inte kodsignerade, så Windows
SmartScreen kan varna. Välj *Mer information* och *Kör ändå*.

## Verifierat och återstående

- Rust-tester, bland dem ett nytt för att skilja på deltagarna igen utan att röra ”Jag”.
  Uppdelningen kördes på 10 minuter riktigt tal med 1, 2 och gissat antal talare: samma resultat
  som förut, på en tredjedel av tiden.
- UI-test (mockade data) för hela flödet: antal från deltagarlistan, uppdelning i bakgrunden,
  namn, lyssna, slå ihop och ångra, dela upp igen, och att text du ändrat under tiden aldrig skrivs
  över. Möten, paus, uppdateringar, sammanfattningar och Textklipp testade igen. Svelte/TypeScript:
  0 fel.
- Båda installationsprogrammen är signerade och signaturerna kontrollerade mot nyckeln i appen.
- Inte provat: ett riktigt digitalt möte med flera deltagare.

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
