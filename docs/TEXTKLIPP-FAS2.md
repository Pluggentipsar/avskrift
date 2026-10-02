# Textklipp fas 2 – videoprojekt och import

Status för [planens](TEXTKLIPP-PLAN.md) fas 2. Mätt lokalt 2026-10-02, RTX 5070 Ti,
Core Ultra 7 265KF, Windows.

## Beslut: egen projektlagring

Planen föreslog en ny arbetstyp i `jobs.rs`. Textklipp-projekt sparas i stället i en
**egen mapp per projekt** under appdata: `textklipp/<id>/` med `project.json`,
`audio16k.wav` och `proxy.mp4`. Skäl: ett projekt består av stora filer som ska
försvinna tillsammans, och den delade `Job`-strukturen och historiken påverkas inte.
Originalfilen ändras aldrig.

## Klart

- **`crates/textklipp`** (ren logik, 9 tester):
  - `media.rs`: medieinfo från ffprobe – längd, bild (kodek, storlek, bildfrekvens,
    variabel bildfrekvens, rotation; omslagsbilder ignoreras), ljud, diskbehov.
  - `ffmpeg.rs`: kommandon för 16 kHz-ljud och proxy; H.264-kodare i ordningen NVENC, AMF,
    Quick Sync, Media Foundation (ingen x264 – LGPL); förlopp från `-progress`.
  - `edl.rs`: klipplistan sparar bara avsikt – strukna ord/ljudblock (stabila id),
    manuella tidsintervall och en pausgräns. Klippintervall räknas fram från ordtider och
    en klippunktsfunktion; `to_edited` räknar om källtid till redigerad tid.
- **Proxy:** 720p H.264, konstant bildfrekvens (källans nominella, så mobilvideo med
  variabel bildfrekvens får en stabil tidslinje), nyckelbild var 0,5 s, AAC, faststart,
  rotation tillämpad. Kodaren provkodas en gång per session; här fungerar NVENC och Media
  Foundation, inte AMF/Quick Sync (finns ej).
- **`src-tauri/src/textklipp.rs`:** projektlagring (atomisk skrivning), FFmpeg utan
  konsolfönster, avbrytning dödar FFmpeg, import i steg med sparning efter varje steg.
  Avbrott före transkript tar bort projektet; under proxy behålls transkriptet och proxyn
  kan göras senare. Proxyfel registreras på projektet utan att texten går förlorad.
- **Kommandon:** `textklipp_probe`, `textklipp_import`, `textklipp_make_proxy`,
  `textklipp_list`, `textklipp_open`, `textklipp_save_edits`, `textklipp_delete`.
- **FFmpeg-sökväg:** medföljande `ffmpeg/` bredvid resurserna, annars källträdet,
  annars `PATH` (utveckling).

## Mätningar

Egen inspelning, 32,4 min, 1080p50 H.264/AAC, 1,4 GB (`import_real_video`, opt-in):

| Steg | Tid |
| --- | --- |
| Ljud ut (16 kHz) | 1,5 s |
| KB-Whisper large, laddning (Vulkan, första gången) | 45 s |
| Transkribering | 57 s |
| Exakta ordtider (DirectML) | 6 s |
| Proxy 720p (NVENC) | 83 s |
| **Totalt** | **192 s** |

Resultat: 4 141 ord med exakta tider, 55 ljudblock, 541 pauser, proxy utan fel.
Projektet tar 370 MB (proxy 310, ljud 59, projektfil 0,6) = 26 % av originalet –
mer än planens uppskattning på 10–15 %; `MediaInfo::working_bytes` räknar försiktigt
(670 MB för denna fil).

Avbrott under proxy (`proxy_cancel_stops_ffmpeg`, opt-in): FFmpeg stoppat 0,6 s efter
avbrottet, ingen halvfärdig fil, ingen kvarvarande process. Ordinarie svit: 112 passerade.

## Återstår i fas 2

1. **LGPL-FFmpeg i paketet.** Under utveckling används installerad FFmpeg (gyan.dev,
   GPL) – den får inte skickas med. Behöver en LGPL-build (t.ex. BtbN `win64-lgpl`),
   låst version + SHA-256, licenstext i NOTICE och paketeringssteg.
2. Ljudfiler utan bild fungerar i importen (ingen proxy) men är inte provade.
3. Variabel bildfrekvens och roterad mobilvideo är bara enhetstestade, inte provade
   på riktiga filer.
