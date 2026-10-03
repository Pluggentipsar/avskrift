"""Write the updater manifests for one release into REL/updates/: <channel>-<variant>.json.
Usage: make_manifests.py VERSION REL_DIR RELEASE_NOTES.md CHANNEL[,CHANNEL]
A pre-release goes to the beta channel only; a stable release to both stable and beta."""
import datetime, json, os, re, sys

version, rel, notes_md, channels = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4].split(',')
text = open(notes_md, encoding='utf-8').read()
section = re.search(r'^## Nytt\s*\n(.*?)(?=^## )', text, re.S | re.M)
notes = (section.group(1) if section else text).strip()
notes = re.sub(r'\*\*(.+?)\*\*', r'\1', notes)  # the dialog shows plain text
notes = re.sub(r'\n +', ' ', notes)  # undo markdown line wraps: one line per bullet
date = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace('+00:00', 'Z')
os.makedirs(os.path.join(rel, 'updates'), exist_ok=True)
for variant in ('vulkan', 'cpu'):
    setup = f'Avskrift_{version}_x64-setup-{variant}.exe'
    signature = open(os.path.join(rel, setup + '.sig'), encoding='utf-8').read().strip()
    manifest = {
        'version': version,
        'notes': notes,
        'pub_date': date,
        'platforms': {'windows-x86_64': {
            'signature': signature,
            'url': f'https://github.com/Pluggentipsar/avskrift/releases/download/v{version}/{setup}',
        }},
    }
    for channel in channels:
        path = os.path.join(rel, 'updates', f'{channel}-{variant}.json')
        json.dump(manifest, open(path, 'w', encoding='utf-8'), ensure_ascii=False, indent=2)
        print(path)
