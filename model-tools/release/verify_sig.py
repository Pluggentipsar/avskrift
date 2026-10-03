"""Verify Tauri updater signatures (minisign) the way the app will: file + .sig against the public key
in tauri.conf.json. Usage: verify_sig.py tauri.conf.json FILE [FILE...] (each FILE has FILE.sig)."""
import base64, hashlib, json, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


def b64lines(text):
    return base64.b64decode(text).decode().splitlines()


conf = json.load(open(sys.argv[1], encoding='utf-8'))
pub_lines = b64lines(conf['plugins']['updater']['pubkey'])
pub = base64.b64decode(pub_lines[1])
assert pub[:2] == b'Ed', pub[:2]
key_id, key = pub[2:10], Ed25519PublicKey.from_public_bytes(pub[10:42])
for path in sys.argv[2:]:
    lines = b64lines(open(path + '.sig', encoding='utf-8').read().strip())
    blob = base64.b64decode(lines[1])
    alg, sig_id, sig = blob[:2], blob[2:10], blob[10:74]
    assert sig_id == key_id, f'{path}: signed with another key'
    data = open(path, 'rb').read()
    message = hashlib.blake2b(data, digest_size=64).digest() if alg == b'ED' else data
    key.verify(sig, message)
    trusted = lines[2].removeprefix('trusted comment: ').encode()
    key.verify(base64.b64decode(lines[3]), sig + trusted)
    print(f'OK  {path.split(chr(92))[-1]}  ({alg.decode()}, {len(data) / 2**20:.1f} MB)')
