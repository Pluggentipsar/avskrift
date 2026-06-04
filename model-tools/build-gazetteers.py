"""Bygg .txt-gazetteers för de deterministiska PII-detektorerna från råa öppna data.

Tar en rå export från SCB / Skolverket / Lantmäteriet och skriver rätt .txt-format
(ett namn per rad, #-header behålls) till src-tauri/src/data/. Slutanvändaren behöver
aldrig köra detta – listorna checkas in färdiga. Kör i model-tools/.venv (har openpyxl).

Källor och användning
---------------------
SCB namnstatistik (tilltalsnamn + efternamn), xlsx "namn med minst två bärare":
    python build-gazetteers.py scb-namn  _export/gazetteer/scb-namn-2022.xlsx
    -> skriver fornamn.txt och efternamn.txt

Skolverkets skolenhetsregister (skolenhetsnamn), CSV/JSON-export:
    python build-gazetteers.py skolverket  _export/gazetteer/skolenheter.csv
    -> skriver skolor.txt (bara namn som INTE redan fångas av suffix-regeln)

SCB/Lantmäteriet tätorter/orter, CSV/xlsx:
    python build-gazetteers.py tatorter  _export/gazetteer/tatorter.xlsx
    -> skriver ortnamn.txt

Matchningen i appen är skiftlägeskänslig (kräver inledande versal), så namnen
title-case:as ("ERIK" -> "Erik", "ANNA-KARIN" -> "Anna-Karin").
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "src-tauri" / "src" / "data"

# Tröskel för "vanliga" namn (antal bärare). SCB-listan har en lång svans av sällsynta
# namn som mest ökar risken för övermaskering; vi tar de vanliga. Justera vid behov.
DEFAULT_MIN_BEARERS = 500

# Suffix som rules::skolnamn redan fångar – exakta skolnamn som slutar så här behöver
# INTE ligga i skolor.txt (håll listan liten, för icke-suffix-namn).
SCHOOL_SUFFIXES = (
    "gymnasieskolan", "grundskolan", "gymnasiet", "förskolan",
    "särskolan", "friskolan", "skolan",
)

# Tillåtna tecken i ett namn/ortnamn (svenska + vanliga lånebokstäver, bindestreck,
# apostrof, mellanslag). Filtrerar bort initialer ("A-C"), siffror och skräp.
VALID = re.compile(r"^[A-Za-zÀ-ÖØ-öø-ÿ][A-Za-zÀ-ÖØ-öø-ÿ\-' ]*$")


def titlecase(name: str) -> str:
    """'ERIK' -> 'Erik', 'ANNA-KARIN' -> 'Anna-Karin', 'VON ESSEN' -> 'Von Essen'.

    Versaliserar varje del avgränsad av mellanslag eller bindestreck och behåller
    avgränsaren. Använder str.capitalize() per del så å/ä/ö hanteras rätt.
    """
    out = []
    token = ""
    for ch in name:
        if ch in " -":
            out.append(token.capitalize())
            out.append(ch)
            token = ""
        else:
            token += ch
    out.append(token.capitalize())
    return "".join(out)


def looks_like_initials(name: str) -> bool:
    """True för 'A', 'A-C', 'A B' o.dyl. (varje del högst ett tecken)."""
    parts = re.split(r"[ \-]", name)
    return all(len(p) <= 1 for p in parts)


def clean_terms(pairs: list[tuple[str, int]], min_bearers: int) -> list[str]:
    """Filtrera (namn, antal) -> sorterad, deduplicerad lista title-case:ade namn."""
    seen: dict[str, None] = {}
    for raw, cnt in pairs:
        if cnt < min_bearers:
            continue
        name = str(raw).strip()
        if len(name) < 2 or looks_like_initials(name) or not VALID.match(name):
            continue
        seen[titlecase(name)] = None
    return sorted(seen)


def write_list(path: Path, header: str, terms: list[str]) -> None:
    body = header.rstrip() + "\n\n" + "\n".join(terms) + "\n"
    path.write_text(body, encoding="utf-8")
    print(f"  {path.relative_to(ROOT)}: {len(terms)} rader")


# --- SCB namn -------------------------------------------------------------------

def read_scb_sheet(wb, sheet: str) -> list[tuple[str, int]]:
    ws = wb[sheet]
    out = []
    for i, row in enumerate(ws.iter_rows(values_only=True)):
        if i < 5:  # rad 0–4 = rubriker/metadata, namn i kolumn A, antal i kolumn B
            continue
        name, cnt = row[0], row[1]
        if name and isinstance(cnt, (int, float)):
            out.append((str(name), int(cnt)))
    return out


def cmd_scb_namn(src: Path, min_bearers: int) -> None:
    import openpyxl

    wb = openpyxl.load_workbook(src, read_only=True, data_only=True)
    forn = read_scb_sheet(wb, "Tilltalsnamn kvinnor") + read_scb_sheet(wb, "Tilltalsnamn män")
    efter = read_scb_sheet(wb, "Efternamn")

    forn_terms = clean_terms(forn, min_bearers)
    efter_terms = clean_terms(efter, min_bearers)

    write_list(DATA / "fornamn.txt", FORNAMN_HEADER.format(min=min_bearers), forn_terms)
    write_list(DATA / "efternamn.txt", EFTERNAMN_HEADER.format(min=min_bearers), efter_terms)


# --- Skolverket skolenheter -----------------------------------------------------

def read_name_column(src: Path, candidates: tuple[str, ...]) -> list[str]:
    """Läs en namnkolumn ur CSV (semikolon/komma) eller JSON; prova kolumnnamnen i tur."""
    names: list[str] = []
    if src.suffix.lower() == ".json":
        import json

        data = json.loads(src.read_text(encoding="utf-8"))
        rows = data if isinstance(data, list) else next(
            (v for v in data.values() if isinstance(v, list)), []
        )
        for r in rows:
            for c in candidates:
                if isinstance(r, dict) and r.get(c):
                    names.append(str(r[c]))
                    break
    else:
        import csv

        text = src.read_text(encoding="utf-8-sig")
        delim = ";" if text.splitlines()[0].count(";") >= text.splitlines()[0].count(",") else ","
        reader = csv.DictReader(text.splitlines(), delimiter=delim)
        cols = {c.lower(): c for c in (reader.fieldnames or [])}
        key = next((cols[c.lower()] for c in candidates if c.lower() in cols), None)
        if key is None:
            sys.exit(f"hittade ingen av kolumnerna {candidates} i {src.name}; fanns: {reader.fieldnames}")
        names = [row[key] for row in reader if row.get(key)]
    return names


def cmd_skolverket(src: Path, _min_bearers: int) -> None:
    raw = read_name_column(src, ("Skolenhetsnamn", "skolenhetsnamn", "namn", "name"))
    seen: dict[str, None] = {}
    for name in raw:
        name = name.strip()
        if len(name) < 2 or not VALID.match(name):
            continue
        low = name.lower()
        if low.endswith(SCHOOL_SUFFIXES) or (low.endswith("s") and low[:-1].endswith(SCHOOL_SUFFIXES)):
            continue  # redan fångat av rules::skolnamn
        seen[name] = None  # skolnamn skrivs som de står (egennamn, redan korrekt versaliserade)
    write_list(DATA / "skolor.txt", SKOLOR_HEADER, sorted(seen))


# --- Tätorter / ortnamn ---------------------------------------------------------

def cmd_tatorter(src: Path, _min_bearers: int) -> None:
    if src.suffix.lower() in (".xlsx", ".xlsm"):
        import openpyxl

        wb = openpyxl.load_workbook(src, read_only=True, data_only=True)
        ws = wb.active
        rows = [r for r in ws.iter_rows(values_only=True)]
        # Hitta en kolumn vars rubrik innehåller "tätort"/"ort"/"namn".
        header_idx, col = None, None
        for i, r in enumerate(rows[:15]):
            for j, cell in enumerate(r):
                if isinstance(cell, str) and re.search(r"tätort|ortnamn|\bort\b|namn", cell, re.I):
                    header_idx, col = i, j
                    break
            if col is not None:
                break
        if col is None:
            sys.exit("hittade ingen tätorts-/namnkolumn i xlsx")
        raw = [r[col] for r in rows[header_idx + 1:] if r[col]]
    else:
        raw = read_name_column(src, ("Tätort", "tätort", "ortnamn", "namn", "name"))
    seen: dict[str, None] = {}
    for name in raw:
        name = str(name).strip()
        if len(name) < 2 or looks_like_initials(name) or not VALID.match(name):
            continue
        seen[titlecase(name)] = None
    write_list(DATA / "ortnamn.txt", ORTNAMN_HEADER, sorted(seen))


# --- Headers (behålls överst i varje fil) ---------------------------------------

FORNAMN_HEADER = """\
# Svenska förnamn (tilltalsnamn) – gazetteer för Category::Person.
# Källa: SCB:s namnstatistik, "tilltalsnamn med minst två bärare" (31 dec 2022).
# Genererad av model-tools/build-gazetteers.py (tröskel: minst {min} bärare).
#
# Format: ett namn per rad. Rader som börjar med # och tomma rader ignoreras.
# Matchningen är skiftlägeskänslig (kräver inledande versal) eftersom många namn
# även är vardagsord (Björn, Sten, My, Ros). Träffar får låg score och granskas."""

EFTERNAMN_HEADER = """\
# Svenska efternamn – gazetteer för Category::Person.
# Källa: SCB:s namnstatistik, "efternamn med minst två bärare" (31 dec 2022).
# Genererad av model-tools/build-gazetteers.py (tröskel: minst {min} bärare).
#
# Format: ett namn per rad. #-rader och tomma rader ignoreras. Matchningen är
# skiftlägeskänslig (kräver inledande versal). Träffar får låg score och granskas."""

SKOLOR_HEADER = """\
# Skolnamn – gazetteer för Category::Plats.
# Källa: Skolverkets skolenhetsregister (öppna data).
# Genererad av model-tools/build-gazetteers.py. Namn som slutar på de suffix som
# rules::skolnamn redan fångar (…skolan, …gymnasiet m.fl.) är bortfiltrerade –
# den här listan är för EXAKTA namn som inte följer suffix-mönstret.
#
# Format: ett namn per rad. #-rader och tomma rader ignoreras. Skiftlägeskänslig."""

ORTNAMN_HEADER = """\
# Ortnamn (tätorter) – gazetteer för Category::Plats.
# Källa: SCB:s tätortsregister / Lantmäteriets ortnamn (öppna data).
# Genererad av model-tools/build-gazetteers.py.
#
# OBS: ju större lista, desto högre risk för övermaskering av vanliga ord. Börja
# smått (t.ex. bara de största tätorterna). #-rader/tomma rader ignoreras.
# Matchningen är skiftlägeskänslig (kräver inledande versal)."""

COMMANDS = {
    "scb-namn": cmd_scb_namn,
    "skolverket": cmd_skolverket,
    "tatorter": cmd_tatorter,
}


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("source", choices=COMMANDS, help="vilken källa exporten kommer från")
    p.add_argument("file", type=Path, help="sökväg till den råa exporten (xlsx/csv/json)")
    p.add_argument("--min-bearers", type=int, default=DEFAULT_MIN_BEARERS,
                   help=f"minsta antal bärare för SCB-namn (default {DEFAULT_MIN_BEARERS})")
    args = p.parse_args()
    if not args.file.exists():
        sys.exit(f"filen finns inte: {args.file}")
    print(f"Bygger gazetteer från {args.file.name} ({args.source})…")
    COMMANDS[args.source](args.file, args.min_bearers)
    print("Klart.")


if __name__ == "__main__":
    main()
