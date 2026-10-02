# Textklipp – klipp video genom att redigera texten

Textklipp gör en färdig videofil av en egen inspelning genom att du stryker text i
transkriptet. Det som är struket klipps bort ur filmen. Allt körs lokalt; originalfilen
ändras aldrig.

## Kom igång

1. Hämta **Exakta ordtider** under *Modeller på datorn* (cirka 630 MB, en gång). Utan dem
   hamnar klippen inte mellan orden.
2. Öppna **Textklipp** i menyn och välj en video- eller ljudfil. Appen visar längd,
   upplösning och diskbehov, transkriberar och gör en lätt arbetskopia (proxy) för
   uppspelning. En timmes film tar några minuter.
3. Öppna klippet. Video och text visas sida vid sida.

## Redigera

- **Markera text och tryck Delete** för att ta bort. Markera borttagen text och tryck
  Delete igen för att ta tillbaka den.
- **Ctrl+Z / Ctrl+Y** ångrar och gör om, **mellanslag** spelar och pausar, klick på ett ord
  hoppar dit.
- Uppspelningen hoppar redan över det borttagna, med samma klippunkter som exporten.
- **Snabbverktyg:** korta alla pauser (1,5 / 1 / 0,7 / 0,5 s), ta bort ljudblock
  ("[ljud]"), visa eller dölj borttagen text, återställ allt.
- **Möjliga omtagningar:** meningar som sägs igen inom två minuter föreslås; du väljer
  själv om den tidigare tagningen ska bort.
- **Detaljvyn** visar vågformen kring markören. Dra en röd kant för att flytta ett klipp,
  finjustera en bildruta i taget och lyssna på skarven i loop.
- **Sök** i texten och **gå till tid** (t.ex. "1:30").
- Fördröjning i trådlösa hörlurar kan kompenseras i spelaren så att markeringen följer
  ljudet.

Ändringarna sparas automatiskt.

## Exportera

**Exportera…** gör en ny videofil från originalet i full upplösning. Välj *Hög kvalitet*
eller *Mindre fil*, och om du också vill ha undertexter (SRT, VTT) eller text för den
klippta filmen. Grafikkortets videokodare används när den finns (NVIDIA, AMD, Intel).

- Bild och ljud hålls i exakt synk genom hela filmen och kontrolleras efter exporten.
- Skarvar i pauser får rummets bakgrundsljud i stället för ett hörbart hack.
- En 32 minuters film tog drygt två minuter att exportera på ett RTX 5070 Ti.
- Filen skrivs klar under ett tillfälligt namn; avbryter du blir ingen halv film kvar.

## Bra att veta

- FFmpeg (LGPL) följer med appen; se `NOTICE.md`.
- Klippen kräver ordtider, därför går Pianissimo inte att använda i Textklipp.
- Provat med egna skärm- och kamerainspelningar (1080p, 30 och 50 bilder/s). Mobilvideo
  med varierande bildfrekvens eller rotation är ännu inte provad.
- Projekten ligger i appens datamapp under `textklipp/` och kan tas bort från listan i
  Textklipp.

Teknisk bakgrund: [plan](TEXTKLIPP-PLAN.md), [fas 1](TEXTKLIPP-FAS1.md),
[fas 2](TEXTKLIPP-FAS2.md), [fas 3](TEXTKLIPP-FAS3.md), [fas 4](TEXTKLIPP-FAS4.md).
