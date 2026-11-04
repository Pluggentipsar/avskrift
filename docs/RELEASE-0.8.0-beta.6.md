# Avskrift 0.8.0-beta.6 – pausa möten och arbeten, och tid kvar

Förhandsrelease. Kan installeras med **Sök efter uppdatering** i appen från 0.8.0-beta.3 eller
senare. Allt från tidigare förhandsversioner ingår. Version 0.6.0 ligger kvar som senaste stabila
release.

## Nytt

- **Pausa ett möte.** Pausa inspelningen under en rast i stället för att avsluta mötet och starta
  ett nytt. Pausen spelas inte in, båda ljudspåren fortsätter i takt, och tidräknaren står still.
  Var pausen låg syns som tidsmarkeringar i anteckningarna.
- **Tid kvar.** Långa arbeten visar en stapel med procent och ungefär hur lång tid som är kvar:
  transkribering, när ett möte färdigställs och import och export i Textklipp.
- **Pausa och fortsätta arbeten.** Bredvid Avbryt arbete finns Pausa. Ett pausat arbete stannar
  inom någon sekund, även mitt i en lång fil, och fortsätter där det var. Medan ett arbete är
  pausat väntar diktering och andra arbeten.
- Datorn kan gå i viloläge, till exempel när du fäller ner locket. Arbetet fortsätter när den
  vaknar.

## Hämta och installera

**Har du 0.8.0-beta.3 eller senare installerad:** välj **Sök efter uppdatering** i menyns
statusruta och **Ladda ner och installera**.

Annars:

- **Avskrift_0.8.0-beta.6_x64-setup-vulkan.exe:** installationsprogram för dator med
  kompatibelt grafikkort.
- **Avskrift_0.8.0-beta.6_x64-setup-cpu.exe:** installationsprogram för dator utan Vulkan-stöd.
- **Windows-Vulkan.zip / Windows-CPU.zip:** portabla mappar utan installation (uppdateras inte i
  appen).

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar för hand. Dina arbeten
ligger kvar i appens datamapp. Installationsprogrammen är inte kodsignerade, så Windows
SmartScreen kan varna. Välj *Mer information* och *Kör ändå*.

## Verifierat och återstående

- Rust-tester på vardera CPU och Vulkan, bland dem nya för att pausa, fortsätta och avbryta under
  en paus. En riktig Whisper-körning på grafikkortet med 10 minuter tal pausades i 3 sekunder utan
  några framsteg och gick sedan klart.
- Tester för uppskattningen av tid kvar, och UI-tester (mockade data) för mötespaus, förlopp och
  paus av arbete samt Textklipp, möten, sammanfattningar och grundflödet. Svelte/TypeScript: 0 fel.
- Båda installationsprogrammen är signerade och signaturerna kontrollerade mot nyckeln i appen.
- Inte provat: en riktig mötespaus med ljudenheter.

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
