# Nästa AVskrift: produkt, UX och visuell riktning

2026-09-11. Förslag utifrån kod och befintliga arbetsflöden, inte en genomförd användarstudie
eller granskning av alla skärmbilder i den körande Windows-appen. Inga produktionsvyer ändras
i denna genomgång. Skissen är ett koncept med påhittade exempel och simulerade interaktioner.

## Produktidén

**Fånga ord. Gör dem användbara. Behåll kontrollen.**

Användaren har valt en bred produkt där **Möten, Diktering och Avidentifiering väger lika tungt**.
Bygg tre tydliga ingångar ovanpå ett gemensamt bibliotek, en gemensam redigeringsyta och
gemensam hantering av versioner, källor, sparande och export. Sammanfattning är en förmåga som
fungerar över flera ingångar. Ett möte ska kunna bli protokoll; ett diktat ska kunna bli ett brev;
ett dokument ska kunna få en avidentifierad version utan att originalet ersätts.

Framgång är att användaren får fram ett användbart och kontrollerat resultat med mindre arbete.
Antalet funktioner, AI-anrop och knappar är dåliga mått på produktens värde.

## Det som redan är värt att behålla

- Lokal bearbetning och möjlighet till CPU-drift.
- Diktering i andra program och återhämtning när infogningen misslyckas.
- Kopplingen mellan ljud, ordtidsstämplar och redigerbar text.
- Manuell granskning av maskering, sökbara projekt och valfri dikteringshistorik.
- En återhållsam identitet med vit grund, indigo och ett särpräglat ordmärke.

## Viktigaste fynden

| Prioritet | Observation i dagens kod | Konsekvens och föreslagen förändring |
|---|---|---|
| P1 | `+page.svelte:2685` blandar globala destinationer med flikar för det aktuella transkriptet. Utbudet ändras med aktuell data. | Stabil huvudnavigation för Bibliotek, Möten, Diktering och Avidentifiering. Lokala flikar hör hemma inne i ett arbete. Det ska vara tydligt vad som byter destination och vad som byter vy av samma innehåll. |
| P1 | `saveCurrentJob` vid `+page.svelte:2314` fångar sparfel utan ett synligt beständigt sparbesked. | Visa Sparar / Sparat / Kunde inte spara med försök igen och möjlighet att exportera en lokal kopia. Bekräfta sparande efter lyckad skrivning. Skydda osparade ändringar vid byte/stängning. |
| P1 | `+page.svelte:3969` beskriver avidentifierad text som trygg att klistra in i extern AI. | Visa saklig status: vad har ersatts, vad är granskat och vilken version kopieras? Undvik att en grön markering antyder att alla känsliga uppgifter säkert är borta. |
| P1 | Sammanfattning och frågor returnerar fri text (`summarize.rs`); underbyggda källhänvisningar är inte del av resultattypen. | Varje beslut, åtgärd och faktasvar ska kunna öppna det relevanta källutdraget. Skilj förslag från det användaren har godkänt. Visa när belägg saknas. Detta kräver stöd i datamodell och motor, inte bara dekorativa länkar i UI. |
| P2 | `+page.svelte:4081–4082` använder 11–12 px och `#a6a7ad` på vitt för bland annat rubriker och hjälptext. | Den färgkombinationen ger cirka 2,40:1. Höj kontrasten, minska versaler och ge viktig information normal lässtorlek. Den mörkare befintliga sekundärfärgen `#696a6f` ger cirka 5,40:1 på vitt. |
| P2 | Modellhämtning och modellval återkommer i flera huvudflöden; exponerade detaljer som modell, diarisering och filformat konkurrerar med resultatet. | Gemensam förberedelse vid första start och en diskret panel för prestanda. Använd Automatiskt / Snabbt / Noggrant där en verklig teknisk avvägning finns; behåll explicita modellval under Avancerat. |
| P2 | Kopiera, Kopiera för AI och många exportformat ligger bredvid varandra. | Samla export i en förhandsvisning: välj innehåll/version, granska resultat, välj format. Behåll snabbkopiering som ett tydligt lokalt kommando. |
| P2 | Dikteringsvyn visar många inställningar samtidigt som varje diktat alltid är ett öppet redigeringsfält (`Dictation.svelte`). | Prioritera senaste texten och dess leveransstatus. Visa tidigare diktat som en läsbar lista, öppna redigering vid behov. Samla genvägar och lagringsval i en särskild inställningspanel. |
| P2 | Word-import markerar tabeller men tar inte med tabellinnehållet (`docio.rs:64`). | Visa importens omfattning före bearbetning. Fullständig tabellhantering bör komma före fler importformat för en produkt som används till avidentifiering. |
| P2 | Projektets typ styr vilka fält som sparas; huvudvyn bär många globala tillstånd (`jobs.rs`, `+page.svelte`). | Ett arbete behöver kunna behålla källor, transkript, utkast och granskad version samtidigt. Byt inte identitet eller kasta underlag när användaren går vidare till ett annat verktyg. |

Kontrastjämförelsen avser deklarerade CSS-färger, inte en fullständig mätning av alla renderade
tillstånd. För vanlig text anger W3C normalt minst 4,5:1, med särskilda undantag bland annat
för stor text och inaktiva kontroller. Källa: [WCAG 2.2, kontrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum).

## Tre likvärdiga arbetsflöden

### Möten

Starta → kontrollera mikrofon och mötesljud → spela in och anteckna → granska ett föreslaget
resultat → exportera eller följ upp.

- Ljudnivå och vald ljudkälla visas före inspelning. En begriplig förklaring när en källa är tyst.
- Anteckningar och markörer under mötet får tidsankare. Åtgärder ska kunna skapas manuellt utan AI.
- Efter mötet finns ett samlat arbete med Källor, Anteckningar, Utkast och granskad version.
- Beslut och åtgärder föreslås med belägg, ansvarig och datum när det faktiskt framgår.
- Kontrollera ett påstående genom att öppna källutdraget; spela relevanta sekunder när ljud finns.

### Diktering

Håll och tala eller växla inspelning → se att texten levererades → rätta eller återanvänd vid behov.

- Diskret indikator nära arbetet, tydlig start/stopp och samma kortkommandon som nu.
- Senaste diktatet går snabbt att återfinna. Misslyckad infogning blir en tydlig återhämtningsväg.
- Skilj Tillfälligt från Sparat. Ett arbetsbibliotek får aldrig automatiskt börja lagra alla diktat.
- Personlig ordlista, namn och vanliga fraser. Textbearbetning visas som ett förslag, med originalet kvar.
- Snabb övergång till avidentifiering eller formatering som mejl/anteckning, utan automatisk sändning.

### Avidentifiering

Klistra in eller importera → se vad som kom med → granska träffar → jämför versioner → exportera.

- Originalet och den bearbetade versionen får tydliga etiketter och hålls isär.
- Klicka ett markerat ord och se kategori, föreslagen ersättning och varför det är markerat.
- Granska samma namn konsekvent över dokumentet, med möjlighet att hantera enstaka förekomster.
- Lättillgänglig manuell maskering och ångra. Ingen precision i procent utan validerad kalibrering.
- Separata statusar för automatiskt hittade träffar, användarens beslut och exporterat dokument.

## Navigation och struktur

```text
AVskrift
  Ditt arbete                 Gemensamt bibliotek, fortsätt pågående arbete
  Möten                       Inspelningar och mötesarbeten
  Diktering                   Snabb diktering, senaste texten, valfri historik
  Avidentifiering              Text- och dokumentgranskning
  Mallar                      Återanvändbara format för samtliga ingångar
  Inställningar               Ljud, genvägar, lagring, prestanda och avancerat

Inne i ett arbete
  Titel + verklig sparstatus
  Källor / Anteckningar / Utkast / Granskning (relevanta för arbetet)
  Arbetsyta + kontextpanel vid behov
  Exportera med förhandsvisning
```

Låt användaren anpassa vilken ingång som öppnas vid start. Dikteringens globala genvägar ska
fungera oberoende av vilken del av huvudappen som visas. Ett gemensamt bibliotek betyder
gemensam åtkomst, inte att alla datatyper får samma lagringsregler.

## Visuell riktning och skissplan

Behåll igenkänningen i dagens AVskrift. Ge mer plats åt dokumentet, större läsbar text och färre
samtidiga kontrollgrupper. En stabil vänsternavigation ersätter den föränderliga toppmenyn.
Det karaktäristiska uttrycket ligger i dokumenttypografin och indigofärgen; övriga ytor är lugna.

- Ljus grund `#FFFFFF`, navigationsyta `#F3F3F8`, huvudtext `#242431`, sekundärtext `#626275`,
  indigo `#3A36B0`, granskningsfärg `#865713`. Motsvarande separata färger för mörkt läge.
- Archivo eller systemfont för arbete och kontroller; Instrument Serif för ordmärke och korta
  dokumentrubriker. Brödtext omkring 16 px, sekundär text 13–14 px, justerbar lässtorlek.
- Mått: konsekvent 4/8-px skala, tydliga fokusmarkeringar, lugna 8-px hörn, inga dekorativa diagram.
- Vänsterställd text. Primär åtgärd nära det den påverkar. Paneler visas när de behövs.
- Rymlig/kompakt vy och ett alternativ med enbart sans kan jämföras utan att ändra arbetsflödet.

```text
┌──────────────┬──────────────────────────────────────┐
│ Avskrift     │ Arbetets titel       Sparstatus       │
│              ├──────────────────────────────────────┤
│ Ditt arbete  │ Lokala vyer                           │
│ Möten        ├─────────────────────────┬────────────┤
│ Diktering    │                         │ Källa /    │
│ Avidentif.   │ Dokument / senaste text │ granskning │
│              │                         │ vid behov  │
│ Inställn.    └─────────────────────────┴────────────┤
│              │ Nästa handling                       │
└──────────────┴──────────────────────────────────────┘
```

Planen har granskats mot briefen: möten får inte ta över diktering och avidentifiering.
Därför börjar skissen i ett gemensamt bibliotek med tre lika framträdande ingångar. Jag avstår
från en mätinstrumentliknande startsida med statistik och lika stora statuskort; användarens
pågående texter och konkreta nästa handlingar är innehållet.

## Funktioner som kan göra den stora skillnaden

1. **Källbelagda resultat.** Källankare som faktiskt överlever redigering och omtranskribering,
   inte AI-genererade tidsstämplar. Fungerar för mötesbeslut, frågor och dokumentutdrag.
2. **Samma underlag genom hela arbetet.** Original, råtranskript, rättad text, sammanfattning och
   avidentifierad version finns kvar. En ändring uppströms markerar beroende utkast som inaktuella.
3. **Verklig återhämtning.** Beständig sparstatus, återuppta avbrutet jobb, versionshistorik och
   ångra utan att material från ett annat arbete blandas in.
4. **Bra standardval.** Ett kort lokalt kapacitetstest, begriplig förberedelsestatus och en
   möjlighet att växla mellan snabbhet och noggrannhet. Uppskattad väntan bara när den är underbyggd.
5. **Mallar som kan återanvändas.** Mötesprotokoll, tjänsteanteckning, kort mejl och granskningsprofil.
   Börja med få välgjorda mallar; lägg till organisationshantering när den har verkliga användare.

Ett generellt chattfönster, automatiska externa integrationer och ett stort mallsortiment bör inte
tränga undan sparande, källor, läsbarhet och fungerande huvudflöden i första ombyggnaden.

## Teknisk grund för UX-förbättringarna

- Dela Svelte-vyn i appskal, bibliotek, möten, diktering, dokumentgranskning och gemensamma
  komponenter för källa, sparstatus och export. Separera navigations-, dokument- och jobbstatus.
- Inför en gemensam arbetsmodell med stabila id:n: arbete → källor → revisioner → härledda
  resultat → granskningsbeslut → exporter. Sparade diktat kan knytas till ett arbete; tillfälliga
  diktat fortsätter ligga endast i sessionen.
- Inför versionsankare och strukturerade resultat för AI: påstående, källreferenser, föreslagen
  åtgärd och användarens beslut. Kontrollera referenser mot källan innan de visas som belägg.
- Atomiska skrivningar och migrering som behåller äldre projektfiler. Ett litet lokalt index kan
  införas separat; bibliotekets gränssnitt ska inte vara beroende av en stor datamigrering dag ett.
- Gemensamma designvariabler, tillgängliga kontroller, fönsterskalning och rendering av bara
  synliga delar i stora transkript. Korta animationer ska inte kosta kännbart på en kontorsdator.

## Byggordning

**Steg 1 – begriplighet och tillit:** stabil navigation, designvariabler, läsbarhet, synligt
sparande/sparfel, enhetlig exportförhandsvisning och tydligare lagrings-/kopieringsbesked.
Leverera samtidigt förbättringar för alla tre ingångarna. Migrera inte hela motorn i samma steg.

**Steg 2 – sammanhängande arbete:** gemensam arbetsmodell och versioner, bättre dikteringsåterhämtning,
förbättrad dokumentimport och en sammanhållen granskningsyta.

**Steg 3 – källbelagd hjälp:** strukturerade AI-utkast, källankare, förslag till uppgifter och
återanvändbara mallar. Godkännande av användaren innan något blir en beslutad uppgift eller delas.

## Så avgör vi om det blev bättre

Testa med nybörjare och vana användare på både kontorsdator och GPU-dator. Låt dem genomföra:
ett möte till kontrollerat protokoll, ett diktat med tappat fokus och ett dokument med uppgifter
som behöver maskeras. Mät tid till ett användbart resultat, felsteg, hjälpbehov och om de kan
förklara vad som är sparat, granskat och exporterat. Jämför mot dagens version före målsättning.

Produktkriterier: inga tysta sparfel; tydligt original/utkast/granskad version; slutförbara huvudflöden
med tangentbord och förstoring; en ny användare kan hitta nästa steg. Ingen användardata eller
telemetri behöver skickas externt för att genomföra dessa lokala tester.
