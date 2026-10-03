# Uppdateringar i appen

Från 0.8.0-beta.3 kan en installerad Avskrift uppdatera sig själv (`src-tauri/src/updates.rs`,
`src/lib/UpdateDialog.svelte`, `tauri-plugin-updater`).

## För användaren

- **Sök efter uppdatering** i menyns statusruta. Appen kontaktar GitHub bara då.
- Kanal: *Stabila versioner* eller *Även förhandsversioner* (standard för en förhandsversion).
- Ny version laddas ner, kontrolleras mot Avskrifts signatur och installeras av
  installationsprogrammet (passivt läge). Appen startar om; arbeten i appens datamapp påverkas inte.
- Spärras under mötesinspelning, bakgrundsbearbetning av möten, pågående arbete och diktering.
- Portabla ZIP-kopior (ingen `uninstall.exe` bredvid exe-filen) uppdaterar inte sig själva; dialogen
  länkar till releasen.

## Signering

- Nyckelpar för uppdateringar (minisign), skapat 2026-10-03:
  - privat: `C:\Users\plugg\.tauri\avskrift-updater.key`, krypterad med lösenordet i
    `C:\Users\plugg\.tauri\avskrift-updater.password` (**aldrig i repot**). Bygget läser dem via
    `TAURI_SIGNING_PRIVATE_KEY` (sökväg) och `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; utan lösenordet
    väntar Tauri på en fråga och bygget fastnar.
  - publik: `plugins.updater.pubkey` i `src-tauri/tauri.conf.json`
- **Säkerhetskopiera nyckeln och lösenordet**, gärna lösenordet i en lösenordshanterare. Utan den kan befintliga installationer inte längre
  uppdateras; en ny nyckel kräver att alla installerar en version för hand igen.
- Detta är inte kodsignering för Windows (SmartScreen); det är en separat fråga.

## Uppdateringsfiler

Appen läser `https://github.com/Pluggentipsar/avskrift/releases/download/updates/<kanal>-<variant>.json`
där kanal är `beta` eller `stable` och variant `vulkan` eller `cpu` (bestäms av byggets `vulkan`-feature).
Releasen `updates` är en förhandsrelease som bara bär dessa filer.

Format (Tauris statiska JSON):

```json
{ "version": "0.8.0-beta.3", "notes": "…", "pub_date": "2026-10-03T12:00:00Z",
  "platforms": { "windows-x86_64": { "signature": "<innehållet i .sig>",
    "url": "https://github.com/Pluggentipsar/avskrift/releases/download/v0.8.0-beta.3/Avskrift_0.8.0-beta.3_x64-setup-vulkan.exe" } } }
```

## Release

1. Höj versionen (`package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`).
2. Bygg båda varianterna med `TAURI_SIGNING_PRIVATE_KEY` satt till sökvägen ovan
   (`bundle.createUpdaterArtifacts` ger `…-setup.exe.sig` bredvid installationsprogrammet).
3. Kontrollera signaturerna mot den publika nyckeln innan publicering.
4. Publicera releasen `v<version>` med installationsprogram och ZIP-filer.
5. Skriv `beta-vulkan.json` och `beta-cpu.json` (och vid stabil release även `stable-*.json`) och ladda
   upp dem till releasen `updates` med `gh release upload updates … --clobber`.
