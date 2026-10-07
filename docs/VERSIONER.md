# Versioner

Vad som kommit i varje version, nyast först. Förhandsversioner (beta) är provversioner; senaste
stabila version visas som *Latest* bland [releaserna](https://github.com/Pluggentipsar/avskrift/releases).
Detaljer finns i release-anteckningarna (`docs/RELEASE-*.md`) och stegdokumenten (`docs/ARBETSYTA-STEG-*.md`).

- **Modellhämtning på fler nätverk och sammanfattningar från extern AI (0.8.0-beta.4, förhandsrelease)** —
  hämtning med Windows certifikat och proxy, fortsättning efter avbrott och hämtning via webbläsaren
  som nödutgång; sammanfattningar som markdown med Klistra in från AI, formaterad visning och riktig
  Word-export. Se [release-anteckningarna](RELEASE-0.8.0-beta.4.md).
  [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.8.0-beta.4).

- **Uppdateringar i appen och möten direkt efter stopp (0.8.0-beta.3, förhandsrelease)** — välj
  *Sök efter uppdatering* i menyn så hämtas och installeras nya versioner med ett klick (signerade,
  bara när du ber om det). Efter ett möte visas texten direkt, och bara det som realtidsdelen
  missade transkriberas efteråt. Se [release-anteckningarna](RELEASE-0.8.0-beta.3.md).
  [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.8.0-beta.3).

- **Pianissimo från Klangs egen export (0.8.0-beta.2, förhandsrelease)** — färre fel på långa
  inspelningar, 45–50 gånger snabbare än realtid och ingen lokal förberedelse. Se
  [release-anteckningarna](RELEASE-0.8.0-beta.2.md).
  [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.8.0-beta.2).

- **Textklipp (0.8.0-beta.1, förhandsrelease)** — klipp en egen videoinspelning genom att stryka
  text i transkriptet och exportera en färdig film med exakt synk. Releasen har också en ny
  arbetsyta, egen ingång för Transkribera och installationsprogram för CPU och Vulkan. Se
  [Textklipp](TEXTKLIPP.md) och [release-anteckningarna](RELEASE-0.8.0-beta.1.md).
  [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.8.0-beta.1).

- **Pianissimo (0.7.0-beta.2, förhandsrelease)** — experimentell, valbar CPU-motor för svensk
  transkribering av ljudfiler, möten och diktering, med minneskontroll.
  KB-Whisper är fortfarande standard. Se [Pianissimo i Avskrift](PIANISSIMO.md).
  [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.7.0-beta.2).

- **Mallflöde (0.7.0-beta.1, förhandsrelease)** — stöd för Supportärende och egna dokumentmallar,
  separata redigerbara utkast, källkopior och manuell AI-överlämning. Se
  [demoguide och avgränsningar](MALLFLODE-MVP.md). Lokal modellkvalitet är ännu inte
  godkänd i de nya supportfallen; [testprotokollet](demo-support/VERIFIERING.md) skiljer
  fungerande programflöde från återstående kvalitetsarbete. [Hämta förhandsreleasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.7.0-beta.1).

- **Version 0.6.0 / arbetsyta 9** — ett sammanhängande mötesflöde med ljudtest, kanalval,
  anteckningar, beslut, åtgärder och uppföljning. Byt namn direkt i mötet, fäst eller arkivera
  arbeten och exportera ett samlat mötesunderlag. Svagt mikrofonljud hanteras bättre.
  Se [releasen](https://github.com/Pluggentipsar/avskrift/releases/tag/v0.6.0)
  och [mötesguiden](ARBETSYTA-STEG-9.md).

- **Arbetsyta 8** — lokalt sökindex, snabbare projektlistor och åtaganden, tydlig sökstatus
  och återuppbyggnad av biblioteket. Se [nyheter, tester och mätningar](ARBETSYTA-STEG-8.md).

- **Arbetsyta 7** — strömmande ljudomvandling, avbrytbara förgrundsarbeten och företräde för
  diktering mellan modellsteg. Se [nyheter, tester och gränser](ARBETSYTA-STEG-7.md).

- **Arbetsyta 6** — gemensam modellcache, automatisk GPU-budget, frigöring av inaktiva modeller
  och CPU-reservväg vid återhämtningsbara GPU-fel.
  Se [nyheter, tester och gränser](ARBETSYTA-STEG-6.md).

- **Arbetsyta 5** — tokenbaserad uppdelning av långa AI-underlag, sammanställning i flera
  omgångar och tydligt förlopp med bevarade tidigare resultat vid fel.
  Se [nyheter, tester och gränser](ARBETSYTA-STEG-5.md).

- **Arbetsyta 4** — samlad modellhantering, lugnare transkriptvy, sökning i hela underlaget,
  justerbar textstorlek och rendering av avsnitt nära läsytan för långa möten.
  Se [nyheter, tester och paket](ARBETSYTA-STEG-4.md).

- **Arbetsyta 3** — källutkast för möten, diktatbearbetning med godkännande, original och
  maskerad text bredvid varandra samt egna mallar och granskningsprofiler.
  Se [nyheter och användning](ARBETSYTA-STEG-3.md).

- **Arbetsyta 2** — original och återställbara versioner, sparade manuella maskningar,
  autosparad källtext, sparade diktat i biblioteket och import av Word-tabelltext.
  Se [nyheter, paket och begränsningar](ARBETSYTA-STEG-2.md).
