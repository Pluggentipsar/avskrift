# Textklipp fas 4 – export

Status för [planens](TEXTKLIPP-PLAN.md) fas 4. Mätt lokalt 2026-10-02, RTX 5070 Ti, NVENC.

## Så fungerar exporten

- **Från originalfilen** i full upplösning, med den sparade klipplistan och **samma
  klippunkter som förhandsvisningen** (`preview_with`).
- **Varje behållen bit läses med egen exakt sökning** (`-ss a -t len -i original`) i stället
  för en filtergraf över hela filen, som skulle buffra allt däremellan.
- **Grupper om 24 bitar** kodas till mellanfiler (Matroska, H.264 + **PCM-ljud**). Sist
  fogas de ihop med **bilden kopierad** och ljudet kodat **en gång** till AAC – AAC per
  grupp skulle lägga in kodarens förtystnad vid varje skarv.
- **Varje bit är exakt *k* bildrutor** i både bild och ljud: gränserna avrundas till hela
  bildrutor, bilden görs till exakt *k* rutor (`fps`, `tpad`, `trim=end_frame`), ljudet till
  exakt *k*/fps s (`apad`, `atrim`). Annars fyller FFmpegs `concat` ut ljudet med tystnad när
  bilden blir en ruta längre, och ljudet glider vid varje skarv (se mätningar).
- **Ljudtoning** vid varje skarv: 10 ms när klippet ligger i tystnad, 25 ms när det skär
  genom ljud. Ingen toning i filmens början och slut.
- Kodare: samma val som proxyn (NVENC, AMF, Quick Sync, Media Foundation). *Hög kvalitet*
  (NVENC CQ 19) eller *Mindre fil* (CQ 27).
- Skrivs till ett tillfälligt namn och döps om först när allt är klart; avbrott eller fel
  lämnar ingen halv film och tar bort mellanfilerna.
- **Synkkontroll efteråt** med ffprobe: bild och ljud inom en bildruta och längden som
  planerat.
- **Undertexter** (SRT, VTT) och **text** (.txt) för den klippta filmen: bara behållna ord,
  tider omräknade till den nya tidslinjen, rutor ≤ 84 tecken och ≤ 6 s, två rader.
- **Dialog i editorn** (*Exportera…*): kvalitet, undertexter/text, val av plats, förlopp,
  avbryt, resultat med synkbesked och *Visa i mappen*.

## Mätningar

`model-tools/textklipp-sync-check.py` lokaliserar korta bitar av den exporterade filmens ljud
i originalljudet (korskorrelation) och jämför med var klipplistan säger att de kommer ifrån.

| Film | Bitar | Resultat | Renderingstid | Synk (bild/ljud) | Ljudfel genom filmen |
| --- | --- | --- | --- | --- | --- |
| 3 min, 1080p30, *första versionen* | 50 | 2:34 | 19,6 s | 154,20 / 154,23 s | **glider till −154 ms** |
| 3 min, efter rättningen | 50 | 2:34 | 18,9 s | 153,999 / 153,999 s | **0,0 ms** (56 punkter) |
| 32 min, 1080p50 | 508 | 24:28 | 242 s (6× realtid) | 1468,0 / 1468,0 s | **0,0 ms** (521 punkter) |

Den första versionen klarade den enkla kontrollen (bild och ljud lika långa inom 30 ms)
men ljudet gled i hopp vid bitar som inte var hela bildrutor – därför finns den
genomgående kontrollen. Testklippen: de två första orden, första ordet efter varje mening,
pauser kortade till 0,7 s.

Tester: `textklipp` 18 enhetstester (renderingsplan, undertexter), apptest
`export_real_video` (opt-in), UI-test 21 steg inklusive exportdialogen.

## Återstår

- Parallella grupper (två samtidiga NVENC-sessioner) för ungefär dubbel fart.
- Rumston i stället för tystnad vid förkortade pauser.
- Provning med mobilvideo (variabel bildfrekvens, rotation) och ljudfiler.
