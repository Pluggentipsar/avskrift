# Textklipp – plan för videoeditor i Avskrift

Mål: ladda in en egen video, transkribera den och klippa genom att stryka text.
Strukna ord försvinner ur videon. Resultatet är en färdig videofil. Allt körs lokalt.
Underlag: [prototypen](TEXTKLIPP-PROTOTYP.md).

## Principer

- **Icke-destruktivt.** Originalfilen ändras aldrig. Projektet sparar en klipplista;
  allt kan ångras, även efter omstart.
- **Texten styr, tiden bestämmer.** Användaren arbetar i text. Klippunkter räknas fram
  från exakta ordtider och ljudet, och kan finjusteras manuellt.
- **Förhandsvisa utan rendering.** Spelaren hoppar över strukna partier direkt.
  Rendering sker först vid export.
- **Ärligt om osäkerhet.** Ord med låg tidskonfidens och ställen där klipp kan höras
  markeras i stället för att döljas.

## Fas 1 – Exakta ordtider i appen

Den största risken; allt annat bygger på den.

1. **Rätta ordgrupperingen** i `transcribe.rs` (`vad AI` → `vadAI`) med enhetstest.
2. **Ordjustering i Rust**, ny modul (`src-tauri/src/wordalign.rs` eller egen crate likt
   `crates/pianissimo`):
   - Exportera VoxRex till ONNX med ett skript i `model-tools/`, låst revision och SHA-256.
     Jämför fp32 (1,2 GB) mot int8/fp16 på tidsnoggrannhet innan storlek väljs.
   - Hämtas vid behov via modellinställningarna, som Pianissimo.
   - Emissioner i 20-sekundersbitar med 1 s överlapp; Viterbi portad från prototypen.
   - Justera per Whisper-segment med ±2 s marginal i stället för hela filen på en gång,
     så att otranskriberat tal inte drar orden fel.
   - Normalisera text före justering: tal → ord ("12" → "tolv"), förkortningar,
     tecken utanför vokabulären.
   - Konfidens per ord. Låg konfidens → markeras i UI; klipp där kräver bekräftelse.
   - GPU via DirectML (`DirectML.dll` följer redan med), CPU som reserv. Minnesregler
     via `memory.rs`.
3. **Pausdetektering**: energibaserad tystnad (1 ms upplösning) sparas med transkriptet;
   används för klippunkter och för "korta pauser".
4. **Otranskriberade ljud**: partier med tal-energi men utan ord visas som `[ljud]`-
   block. KB-Whisper rensar ofta bort eh/öh – de blir då ändå strykbara.

**Klart när:** median ≤ 30 ms fel för ordstarter och -slut mot 100 manuellt
markerade gränser i minst tre egna inspelningar (även en med två talare och bakgrundsljud,
och en på 30–60 min). Justering ≤ 60 s för en timme video på GPU.

## Fas 2 – Videoprojekt och import

1. Ny arbetstyp **Video** i `jobs.rs`: källsökväg, medieinfo (ffprobe), transkript med
   justerade ord, pauser och klipplista. Versioner via befintlig versionshantering.
2. **FFmpeg** tas in för video (Symphonia behålls för ljudfiler):
   - Ljud ut till 16 kHz mono för transkribering/justering.
   - Medieinfo: upplösning, bildfrekvens, rotation, variabel bildfrekvens (mobiler).
   - **Proxy** när WebView2 inte kan spela källan (HEVC, ProRes, MOV/PCM, VFR):
     H.264 720p med hårdvarukodare. Klipp räknas alltid mot originalets tider.
3. **Klipplista (EDL)**: strukna ordintervall via stabila ord-id + manuella tidsintervall.
   Klippunkter beräknas, sparas inte som sanning – samma lista ger samma resultat efter
   omjustering.

## Fas 3 – Editorn

Byggs som egna komponenter under `src/lib/editor/` (inte i `+page.svelte`, som redan
är 4 600 rader).

- **Layout:** video + tidslinje till vänster, transkript som redigerbart dokument till höger.
  Talare visas när diarisering är på.
- **Textredigering:** markera ord → Delete stryker. Strukna ord visas överstrukna
  (växla till dolda). Återställ via markering + Ctrl+Shift+Z eller högerklick.
  Texten kan inte skrivas om (ingen röstsyntes).
- **Uppspelning:** klick på ord hoppar dit; aktuellt ord markeras; strukna partier hoppas
  över med `requestVideoFrameCallback` för bildrutenoggrann övergång.
- **Tidslinje:** vågform, ord och klipp. Dra i klippkanter, nudga ±1 bildruta / ±10 ms,
  lyssna på ett klipp i loop (1 s före/efter).
- **Snabbverktyg:** ta bort `[ljud]`-block och utfyllnadsord (redigerbar lista: eh, öh,
  ehm, liksom, alltså …) – alltid som förslag med förhandsgranskning, aldrig tyst.
  Korta pauser längre än X s till Y s. Hitta dubbletter (upprepade tagningar, som i
  testfilmen) och föreslå att behålla den sista.
- **Ångra/gör om** för alla ändringar; autospar via `save-queue.ts`.
- **Kortkommandon:** Mellanslag, J/K/L, Delete, Ctrl+Z/Ctrl+Y, pilar för ord.

## Fas 4 – Export

- Rendering med FFmpeg: `trim`/`atrim` + `concat` för få klipp; för många klipp
  (> 100) segmentvis rendering + konkatenering för att hålla filtergrafen liten.
- Ljud: toning 10 ms som standard, 20–40 ms där klippet ligger i tal.
  Senare: rumston i stället för tystnad vid förkortade pauser.
- Kodare väljs automatiskt: NVENC / AMF / Quick Sync, annars Media Foundation (`h264_mf`).
  Behåll originalets upplösning och bildfrekvens. Förinställningar: Hög kvalitet,
  Liten fil.
- Förlopp via `-progress`, avbrytbart via `work.rs`, synkkontroll efteråt
  (bild/ljud-längd < 1 bildruta).
- Även export av redigerad text och SRT/VTT med omräknade tider.

**Licens:** gyan.dev `full_build` är GPL (libx264). Appen skickar med en **LGPL-byggd
FFmpeg** utan x264 och använder hårdvarukodare/Media Foundation; licenstext i NOTICE.
Följer med paketet (beslut), ca 80–100 MB extra per ZIP.

## Fas 5 – Efter första versionen

Flera klipp i samma projekt, bildspår (B-roll), undertexter inbrända, ljudnormalisering,
export av klipplista till DaVinci/Premiere (EDL/FCPXML), Pianissimo som
transkriberingsmotor när den har ordtider att lita på.

## Risker

| Risk | Hantering |
| --- | --- |
| Klipp mitt i sammanhängande tal hörs | Längre toning; UI föreslår närmaste paus; varningsmarkering. |
| Otranskriberat tal, musik, överlapp | Justering per segment, konfidens, `[ljud]`-block. |
| VFR-video från mobil, rotation | Proxy/normalisering vid import; klipp på originalets tidsbas. |
| Minne: Whisper large + VoxRex | Sekventiell laddning och frigöring via `memory.rs`; CPU-reserv. |
| WebView2 kan inte spela alla format | Proxy. |
| FFmpeg-licens och storlek | LGPL-bygge, hämtning vid behov. |

## Ordning och avstämning

Fas 1 → avstämning med provklipp → Fas 2 och 3 parallellt (motor/UI) → Fas 4 →
förhandsrelease. Varje fas avslutas med tester, mätningar och ett dokument i
`docs/` som för Pianissimo.

## Beslut (2026-10-02)

1. **Egen flik** i appens navigering: *Textklipp*.
2. **FFmpeg följer med paketet** (LGPL-bygge), ingen hämtning vid första import.
3. **Längd: 30–60 minuter** är normalfallet, ibland korta filmer på några minuter.

Konsekvenser av 30–60 minuter:

- **Bakgrundsarbete.** Import, transkribering, justering och proxy körs som avbrytbara
  bakgrundsjobb via `work.rs`, med förlopp per steg. Editorn går att öppna när
  transkriptet finns; justeringen fyller på ordtider löpande.
- **Justering per segment** (redan i Fas 1) är nödvändig – en Viterbi över 60 min
  (180 000 bildrutor) är onödigt stor och känslig för otranskriberade partier.
  Mål: ≤ 60 s för en timme på GPU.
- **Proxy alltid för långa filer.** 60 min 1080p från kamera/mobil är ofta 5–15 GB med
  glesa nyckelbilder; sökning i originalet blir trög. Proxy: 720p H.264 med tät
  nyckelbild (var 0,5 s) så att hopp över klipp blir omedelbara.
- **Transkriptet virtualiseras** i editorn (8 000–10 000 ord per timme): bara synliga
  stycken renderas, sökning och "gå till tid" ersätter skrollning.
- **Export i segment.** Hundratals klipp per timme: rendera sammanhängande bitar parallellt
  och konkatenera, i stället för en enda filtergraf. Kopiera oförändrade långa partier
  utan omkodning där nyckelbilder tillåter (smart render) – senare optimering.
- **Diskplats.** Proxy + 16 kHz-ljud ≈ 10–15 % av originalet; visas före import och
  rensas när projektet tas bort.
- **Testmaterial för Fas 1** ska inkludera minst en inspelning på 30–60 min.
