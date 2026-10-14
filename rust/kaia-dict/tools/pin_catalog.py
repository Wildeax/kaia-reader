"""Development-only catalog builder. Fetch each KOReader Wiktionary URL once."""
import concurrent.futures
from datetime import date
import hashlib
import json
from pathlib import Path
import re
import urllib.request
import urllib.parse

root = Path(__file__).resolve().parents[3]
source = root / 'frontend/ui/data/dictionaries.lua'
entries, skipped = [], []
for block in re.findall(r'\{([^{}]+)\}', source.read_text()):
    item = dict(re.findall(r'(name|lang_in|lang_out|url|license)\s*=\s*(?:_\()?"([^"]+)"', block))
    if 'url' not in item:
        continue
    if not item.get('license') or 'unknown' in item['license'].lower():
        skipped.append({**item, 'reason': 'KOReader does not state a clear license.'})
    elif 'wiktion' in block.lower():
        item['id'] = 'wiktionary-' + hashlib.sha256(item['url'].encode()).hexdigest()[:16]
        entries.append(item)

def pin(item):
    try:
        request = urllib.request.Request(urllib.parse.quote(item['url'], safe=':/%?=&'), headers={'User-Agent': 'Kaia-catalog-development'})
        with urllib.request.urlopen(request, timeout=90) as response:
            digest, size = hashlib.sha256(), 0
            magic = b''
            while chunk := response.read(1024 * 1024):
                if not magic:
                    magic = chunk[:4]
                digest.update(chunk)
                size += len(chunk)
        if magic[:2] != b'\x1f\x8b' and magic != b'\x28\xb5\x2f\xfd' and magic[:2] != b'PK':
            raise ValueError('Response is not a gzip, zstd or zip archive')
        result = {**item, 'size': size, 'sha256': digest.hexdigest()}
        print('PIN', item['name'], size, flush=True)
        return True, result
    except Exception as error:
        print('SKIP', item['name'], str(error), flush=True)
        return False, {**item, 'reason': 'Could not pin the listed URL: ' + str(error)}

if __name__ == '__main__':
    pinned = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        for ok, result in pool.map(pin, entries):
            (pinned if ok else skipped).append(result)
    manifest = {'source': 'frontend/ui/data/dictionaries.lua',
                'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                'pinned_on': date.today().isoformat(), 'entries': pinned, 'skipped': skipped}
    (root / 'resources/kaia-dict/catalog.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
    print('TOTAL pinned', len(pinned), 'skipped', len(skipped), flush=True)
