# Textklipp – prototyp för textbaserad videoredigering

Fråga: går det att klippa bort ord i en video genom att stryka dem i transkriptet,
utan att klippen hörs? Prototypen jämför KB-Whispers egna ordtider med
forced alignment mot en svensk wav2vec2-modell. Ingen appkod ändrades.

Lokalt 2026-10-02, RTX 5070 Ti + Core Ultra 7 265KF, Windows. Testmaterial: egen
inspelning, 194,6 s, 1920×1080, 30 fps, H.264/AAC 44,1 kHz stereo. Innehåller prat
före inspelningen, tre tagningar av samma presentation och avbrutna meningar.
Filmen och alla resultat ligger lokalt under `.build-tools/textedit/` och checkas inte in.

## Verktyg

| Fil | Gör |
| --- | --- |
| `model-tools/textedit-probe/` | KB-Whisper via samma whisper-rs 0.14.4 som appen; ordtider grupperas exakt som i `transcribe.rs`. Bygg med `--features vulkan`. |
| `model-tools/textedit-align.py` | CTC forced alignment av Whisper-texten mot `KBLab/wav2vec2-large-voxrex-swedish` (CC0-1.0, revision `ca70e31c06a2617bf7fe3b4fb5d387d2b19b2983`). Egen Viterbi i numpy, så att algoritmen kan portas till Rust. |
| `model-tools/textedit-render.py` | Stryker ord/fraser och renderar videon med FFmpeg (`trim`/`atrim` + `concat`, NVENC). Läge `whisper` = klipp rakt på Whispers ordtider. Läge `aligned` = klipp vid den tystaste bildrutegränsen mellan justerade ord + 10 ms ljudtoning. |

```powershell
ffmpeg -i VIDEO -vn -ac 1 -ar 16000 -c:a pcm_s16le audio16k.wav
avskrift-textedit-probe.exe kb-whisper-large.bin audio16k.wav whisper.json gpu
python model-tools/textedit-align.py VOXREX_DIR audio16k.wav whisper.json aligned.json
python model-tools/textedit-render.py VIDEO audio16k.wav aligned.json edits.json ut.mp4 aligned
```

## Resultat

- **Hastighet.** KB-Whisper large på GPU (Vulkan): 5,6 s för 194,6 s ljud, 423 ord.
  wav2vec2-emissioner på CPU i 20-sekundersbitar: 16,1 s. Viterbi: 0,6 s.
- **Whispers ordtider räcker inte för klippning.** Median 0,31 s från de justerade tiderna,
  på flera ställen 3–4 s. Ord ligger kant i kant och sväljer pauser ("rullar" 1,2 s);
  "Vad är AI…" lades på 19,0 s där ljudet är tyst – talet börjar vid 22,8 s.
  Segmenttiderna landar alltid på hela sekunder.
- **Justeringen träffar talet.** Andel ljudrutor över brusgolv+12 dB inom orden:
  Whisper 0,86, justerat 0,93.
- **Men CTC-ord är för korta.** Ordslut hamnar tidigt (CTC ger toppar, inte ordgränser);
  bara 23 % av mellanrummen ≥150 ms mellan justerade ord var tysta. Klipp ska därför
  läggas vid tystaste punkten *mellan* ord, inte vid ordgränsen.
- **Klippunkter i ljud** (nivå ±15 ms runt klippet > brusgolv+12 dB), samma sex klipp:
  Whisper 11 av 12, justerat 6 av 12. De återstående sex ligger främst där ord flyter
  ihop utan paus ("vad AI egentligen är", "du faktiskt behöver").
- **Synk.** Bild/ljud-längd efter klipp: justerat 100,900/100,929 s (< 1 bildruta),
  Whisper 107,533/107,599 s.

## Fynd att åtgärda

- **Ordgruppering i appen:** "vad AI" blev ett ord, `vadAI`, med samma logik som
  `transcribe.rs`. Påverkar redan ordmarkering i Avskrift.
- **Siffror:** "12" justeras mot tecknen 1 och 2, inte mot det uttalade "tolv".
  Text måste normaliseras (tal → ord) före justering.
- **Ej transkriberat tal** (t.ex. 157–178 s där en annan film spelas, tveksamma partier i
  början, låg poäng < 0,3) ger osäkra tider. Behöver konfidens och reservväg.
- **KB-Whisper rensar ofta bort utfyllnadsord** (eh, öh). Det som inte står i texten går
  inte att stryka – se planen.

Bedömning av hörbarhet görs av människa: jämför `klipp-aligned.mp4` och
`klipp-whisper.mp4` i `.build-tools/textedit/`. Ett enda testklipp, en talare,
tyst rum – ingen generell kvalitetsgaranti.
