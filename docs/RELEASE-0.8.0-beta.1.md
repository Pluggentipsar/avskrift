# Avskrift 0.8.0-beta.1 – Textklipp och ny arbetsyta

Förhandsrelease med **Textklipp**, ett nytt sätt att klippa video genom att redigera texten,
och en omgjord arbetsyta. Allt från 0.7.0-beta.2 (Pianissimo, mallflöde) ingår, med samma
kända begränsningar. Version 0.6.0 ligger kvar som senaste stabila release.

## Nytt

- **Textklipp:** välj en egen inspelning, stryk text i transkriptet och exportera en färdig
  videofil där det strukna är bortklippt. Förhandsvisning utan rendering, korta pauser med
  ett klick, förslag på omtagningar, detaljvy med vågform och dragbara klippkanter,
  undertexter (SRT/VTT) till den klippta filmen. Bild och ljud hålls i exakt synk; skarvar i
  pauser får rummets bakgrundsljud. Se [Textklipp](TEXTKLIPP.md).
- **Exakta ordtider** (cirka 630 MB, hämtas under *Modeller på datorn*): ordgränser från en
  svensk wav2vec2-modell (KBLab VoxRex), som Textklipp behöver för klipp mellan orden.
- **FFmpeg 8.1 (LGPL)** följer med appen för Textklipp. Licensen finns i `NOTICE.md` och
  `ffmpeg/LICENSE-FFmpeg.txt`.
- **Transkribera** är en egen ingång i menyn och på startsidan, skild från Möten.
- **Nytt utseende:** lugnare färgskala (grafit med grön accent och gul överstrykning för
  ordet som spelas), meny grupperad i Skapa, Bearbeta och Hitta, och en startsida med
  sökfält, ingångar och *Fortsätt där du slutade*.
- **Arbetsytan:** en kompakt rubrikrad med en primär knapp (*Exportera…*), mindre vanliga
  vyer under *Mer*, uppspelningsraden alltid längst ned och en högerspalt med
  sammanfattning, beslut och åtgärder bredvid transkriptet.

## Rättat

- Text i Avidentifiering och i lokala sammanfattningar gick inte att scrolla till.
- Ord som avslutades av en token med bara mellanslag slogs ihop med nästa ord.

## Hämta och installera

- **Avskrift_0.8.0-beta.1_x64-setup-vulkan.exe:** installationsprogram, GPU-version för dator
  med kompatibelt grafikkort. Whisper och de lokala språkmodellerna körs på grafikkortet.
- **Avskrift_0.8.0-beta.1_x64-setup-cpu.exe:** installationsprogram för dator utan
  Vulkan-stöd eller när CPU-körning önskas.
- **Windows-Vulkan.zip / Windows-CPU.zip:** samma sak som portabla mappar utan installation.
  Packa upp hela ZIP-filen till en ny mapp och starta `avskrift.exe`.

Stäng den gamla Avskrift (även i meddelandefältet) innan du installerar. Dina arbeten ligger
kvar; de sparas i appens datamapp, inte i programmappen. Talmodeller, Pianissimo, språkmodeller
och Exakta ordtider hämtas vid behov i appen och ingår inte i paketen.

Installationsprogrammen är inte kodsignerade, så Windows SmartScreen kan varna. Välj
*Mer information* och *Kör ändå*.

## Verifierat och återstående

- 112 ordinarie Rust-tester passerade på vardera CPU och Vulkan (release-profil); 30 opt-in-tester
  som kräver riktiga modeller eller videofiler ingick inte i den ordinarie sviten.
- Svelte/TypeScript: 0 fel, 3 tidigare varningar. UI-testerna för arbetsytan, Transkribera,
  scrollning, Textklipp (21 steg) och grundflödet passerade mot mockade data.
- Textklipps export är mätt på egna inspelningar (3 och 32 min, 1080p): bild och ljud i synk
  med 0,0 ms avvikelse genom hela filmen. Se `docs/TEXTKLIPP-FAS4.md`.
- Båda versionerna byggda med optimerad whisper.cpp (Ninja, /O2). Varje exe- och DLL-fil
  kontrollerades mot sina beroenden; inga saknades. CPU-versionen har inget Vulkan-beroende.
  Installationsprogrammen innehåller FFmpeg, modellerna för avidentifiering och
  talarseparering samt alla körbibliotek.
- Installationsprogrammen är inte provinstallerade i detta bygge. Textklipp är inte provat med
  mobilvideo (varierande bildfrekvens, rotation).

SHA256SUMS.txt innehåller kontrollsummor för alla filer i releasen.
