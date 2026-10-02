# Tredjepartskomponenter i Avskrift

Avskrift distribueras med, eller hämtar vid behov, följande komponenter från andra.
Modeller som hämtas i appen listas också i *Modeller på datorn*.

## FFmpeg (medföljer, mappen `ffmpeg/`)

Används av Textklipp för att läsa video, ta ut ljud och skapa uppspelningskopior.

- Version: FFmpeg `n8.1.3-14-g330caae0c1`, Windows x64, delade bibliotek.
- Bygge: [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds), release
  `autobuild-2026-10-01-13-06`, fil `ffmpeg-n8.1.3-14-g330caae0c1-win64-lgpl-shared-8.1.zip`,
  SHA-256 `bf545d8fee9bb6957c1f3dea0f384bf64edead407d763326dbbd2de1b04768a4`.
- Licens: **GNU LGPL version 3 eller senare** (`--enable-version3`, utan `--enable-gpl` och
  `--enable-nonfree`; x264 och x265 ingår inte). Licenstexten följer med som
  `ffmpeg/LICENSE-FFmpeg.txt`.
- Källkod: FFmpeg på [git.ffmpeg.org](https://git.ffmpeg.org/ffmpeg.git), commit `330caae0c1`
  (taggen `n8.1.3` plus 14 commits); byggskript och beroenden i BtbN-repot ovan.
- Avskrift länkar inte mot FFmpeg utan startar `ffmpeg.exe`/`ffprobe.exe` som separata
  program. Biblioteken (`av*.dll`, `sw*.dll`) kan bytas mot en egen kompatibel version.
- Hämtas och kontrolleras med `model-tools/fetch-ffmpeg.ps1`.

## Modeller

- **KBLab/wav2vec2-large-voxrex-swedish** (exakta ordtider), CC0 1.0. ONNX-export i halv
  precision publicerad som releasen `models-wordalign-1`; se `docs/TEXTKLIPP-FAS1.md`.
- **Pianissimo** (KlangAI, CC BY 4.0), KlangAI:s egen ONNX-export: se
  `crates/pianissimo/NOTICE.md`.
