# Textklipp fas 3 – editorn

Status för [planens](TEXTKLIPP-PLAN.md) fas 3. Mätt lokalt 2026-10-02.

## Klart

- **Egen flik *Textklipp*** i navigeringen och på startsidan (`src/lib/textklipp/`).
  - *Startvy:* välj video eller ljudfil, se längd, upplösning, bildfrekvens, storlek och
    diskbehov, välj talarseparering, transkribera och öppna. Lista över klipp med
    öppna/ta bort (originalet påverkas aldrig). Varning om *Exakta ordtider* saknas och
    spärr för Pianissimo, som inte ger ordtider.
  - *Editor:* video (proxy) och transkript sida vid sida. Markera text och tryck
    **Delete** för att ta bort; markera borttagen text och tryck **Delete** igen för att
    återställa. **Ctrl+Z / Ctrl+Y** ångrar och gör om, **mellanslag** spelar och pausar,
    klick på ett ord hoppar dit. Aktuellt ord markeras under uppspelning.
  - *Förhandsvisning utan rendering:* uppspelningen hoppar över borttagna partier.
    Klippunkterna räknas av backend (`textklipp_preview`) med samma regel som exporten
    kommer att använda – tystaste bildrutegräns mellan behållna och borttagna ord – så
    det man hör är det man får.
  - *Tidslinje* med borttagna partier och uppspelningsmarkör; klick hoppar.
  - *Snabbverktyg:* korta pauser (1,5 / 1 / 0,7 / 0,5 s), ta bort alla ljudblock,
    återställ allt, visa eller dölj borttagen text.
  - *Ljudblock* visas i löptexten där de hörs. Med hög säkerhet (≥ 0,8) visas vad
    modellen hörde, t.ex. "[faktiskt]" där KB-Whisper tappat ordet; annars "[ljud]".
    Modellens ordgränser ger nu mellanslag ("och så där").
  - *Läsbara stycken:* yttranden slås ihop medan samma talare fortsätter (paus < 1,2 s,
    högst 90 ord per stycke).
  - Klipplistan sparas automatiskt (0,7 s efter senaste ändring) och när man lämnar.
- **`Loudness` strömmande:** 1 f32 per ms (~14 MB per timme) i stället för en
  prefixsumma över alla prover (~460 MB per timme). Samma pauser som tidigare.

## Tester

- `model-tools/ui-textklipp.cjs` (Playwright, Edge, mockad Tauri-IPC, riktig proxy och
  riktiga justerade ord från en egen inspelning): 12 steg passerar – öppna, stryka ord,
  ångra/gör om, stryka ett intervall, återställa, uppspelning som börjar efter
  bortklippt intro och **aldrig spelar ett struket ord** (videotiden kontrolleras varje
  bildruta genom klippet), ordmarkering, dölja borttaget, korta pauser, ta bort alla
  ljudblock, återställ allt. Skärmbilder sparas i `.build-tools/` eftersom de visar en
  egen inspelning.
- `model-tools/ui-textklipp-perf.cjs` på 32-minutersprojektet (4 141 ord, 55 ljudblock):
  öppna 143 ms; stryka 1 / 101 / 401 ord: 31 / 28 / 19 ms.
- Rust 112 + wordalign 10 + textklipp 9 enhetstester; Svelte/TypeScript 0 fel.

```powershell
node model-tools/ui-textklipp-fixture.cjs WHISPER.json ALIGNED.json .build-tools/textedit/ui-fixture.json
$env:AVSKRIFT_PLAYWRIGHT='…\node_modules\playwright'; $env:AVSKRIFT_TK_FIXTURE='…'; $env:AVSKRIFT_TK_PROXY='…'
node model-tools/ui-textklipp.cjs   # mot npm run dev
```

## Återstår i fas 3

1. Provning i den riktiga appen (inte bara mockad IPC) med import av egen film.
2. Vågform i tidslinjen och dra i klippkanter / nudga ±1 bildruta.
3. Sök i texten och "gå till tid".
4. Hitta upprepade tagningar och föreslå att behålla den sista.
5. Lyssna på ett klipp i loop (1 s före/efter).
