#!/usr/bin/env python3
"""Compare the pinned KOReader Lua wrapper with kaia-doc, without downloads."""
import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import zlib

REPO = Path(__file__).resolve().parents[2]
# Allow one gray level of rounding per pixel, but no displaced glyph edges.
# Both sides use the same native libraries and font bytes on each host.
MAX_DELTA = 1
MAX_MAE = 0.01


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def png(path, pixels, width, height):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    scanlines = b''.join(b'\0' + pixels[y * width:(y + 1) * width] for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 0, 0, 0, 0))
                     + chunk(b'IDAT', zlib.compress(scanlines)) + chunk(b'IEND', b''))


def image_diff(a, b, output, width, height):
    left, right = a.read_bytes(), b.read_bytes()
    if len(left) != width * height or len(right) != width * height:
        raise ValueError('wrong image dimensions')
    delta = bytes(abs(x - y) for x, y in zip(left, right))
    # White means equal; dark pixels show differences amplified sixteen times.
    png(output, bytes(255 - min(255, d * 16) for d in delta), width, height)
    maximum, mae = max(delta), sum(delta) / len(delta)
    return dict(max_delta=maximum, mean_absolute_error=mae,
                changed_pixels=sum(d != 0 for d in delta),
                passed=maximum <= MAX_DELTA and mae <= MAX_MAE)


def compare(left, right, folder, width, height):
    differences, images = [], []
    if left['page_count'] != right['page_count']:
        differences.append(f"page count: KOReader={left['page_count']} Kaia={right['page_count']}")
    a = {p['page']: p for p in left['pages']}
    b = {p['page']: p for p in right['pages']}
    if not a or not b:
        differences.append('empty page samples')
    for page in sorted(a.keys() | b.keys()):
        if page not in a or page not in b:
            differences.append(f'page {page}: sample exists on only one side')
            continue
        for field in ('start', 'end', 'text'):
            if a[page][field] != b[page][field]:
                differences.append(f'page {page}: {field} differs')
                diff = ''.join(difflib.unified_diff(a[page][field].splitlines(True),
                                                  b[page][field].splitlines(True),
                                                  fromfile='KOReader', tofile='Kaia'))
                (folder / f'page-{page}-{field}.diff').write_text(diff, encoding='utf-8')
        filename = f'page-{page}.gray'
        result = image_diff(folder / 'koreader' / filename, folder / 'kaia' / filename,
                            folder / f'page-{page}-diff.png', width, height)
        images.append(dict(page=page, **result))
        if not result['passed']:
            differences.append(f'page {page}: image differs')
    return differences, images


def run(args):
    start = time.monotonic()
    deadline = start + 840  # Leave a minute for reports within the CI step.
    output = args.output.resolve()
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        print(f'ERROR: output directory must be empty: {output}', file=sys.stderr)
        return 1
    output.mkdir(parents=True, exist_ok=True)
    report = dict(thresholds=dict(max_delta=MAX_DELTA, max_mae=MAX_MAE,
                                 text='exact', anchors='exact', page_count='exact'), cases=[])
    summary = []

    def command(argv, cwd, log):
        with log.open('wb') as stream:
            subprocess.run(argv, cwd=cwd, env=os.environ, stdout=stream, stderr=subprocess.STDOUT,
                           check=True, timeout=max(1, deadline - time.monotonic()))

    try:
        emulators = sorted(REPO.glob('koreader-emulator-*/koreader'))
        if args.emulator is None and len(emulators) != 1:
            raise ValueError('select one emulator with --emulator')
        emulator = (args.emulator or emulators[0]).resolve()
        manifest = json.loads((REPO / 'tools/kaia/cre-corpus/manifest.json').read_text())
        report['corpus'] = manifest
        report['git_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
        report['platform'] = sys.platform
        for name, directory in [('base_commit', REPO / 'base'), ('test_commit', REPO / 'test')]:
            report[name] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=directory, text=True).strip()
        # Linux modules depend on other libraries beside libkoreader-cre.
        os.environ['LD_LIBRARY_PATH'] = str(emulator / 'libs') + os.pathsep + os.environ.get('LD_LIBRARY_PATH', '')
        fonts = sorted(p for p in (emulator / 'fonts').rglob('*')
                       if p.suffix.lower() in ('.ttf', '.otf', '.ttc') and 'urw' not in p.parts
                       and not (p.name == 'symbols.ttf' and p.parent.name == 'nerdfonts'))
        if not fonts:
            raise ValueError('no emulator fonts found')
        report['resources'] = {str(p.relative_to(emulator)): sha(p) for p in fonts}
        report['resources'].update({str(p.relative_to(emulator)): sha(p)
                                   for p in sorted((emulator / 'data').rglob('*')) if p.is_file()})
        report['reference_libraries'] = {p.name: sha(p) for p in (emulator / 'libs').glob('*koreader-cre*') if p.is_file()}
        # Keep comparison build outputs separate from other workspace builds.
        os.environ['CARGO_TARGET_DIR'] = str(Path.home() / 'kaia-wt-cre-target')
        command(['cargo', 'build', '--locked', '-p', 'kaia-doc', '--example', 'cre_snapshot'],
                REPO / 'rust', output / 'build.log')
        binary = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/examples/cre_snapshot'
        base = dict(width=1264, height=1680, font_family='Noto Serif', font_size=47,
                    margin_left=22, margin_right=22, margin_top=32, margin_bottom=32,
                    line_percent=100, hyphenation='English_US.pattern', embedded_styles=True,
                    font_weight=400, css_extra='', font_files=[str(p) for p in fonts])
        profiles = [('reader', {}), ('no-hyphenation', dict(hyphenation='@none')),
                    ('sans-large', dict(font_family='Noto Sans', font_size=55, line_percent=120,
                                        font_weight=500, embedded_styles=False,
                                        margin_left=48, margin_right=48, margin_top=40, margin_bottom=40))]
        for book in manifest['books']:
            path = REPO / book['path']
            if sha(path) != book['sha256'] or path.stat().st_size != book['size']:
                raise ValueError(f'corpus hash/size changed: {path}')
            for profile, overrides in profiles:
                settings = dict(base, **overrides)
                name = f'{path.stem}-{profile}'
                folder = output / name
                folder.mkdir(exist_ok=True)
                settings_path = folder / 'settings.json'
                settings_path.write_text(json.dumps(settings, indent=2) + '\n')
                for side in ('koreader', 'kaia'):
                    (folder / side).mkdir(exist_ok=True)
                command([str(emulator / 'luajit'), str(REPO / 'rust/kaia-doc/tests/koreader_layout.lua'),
                         str(path), str(settings_path), str(folder / 'koreader')], emulator, folder / 'koreader.log')
                command([str(binary), str(emulator), str(path), str(settings_path), str(folder / 'kaia')],
                        REPO, folder / 'kaia.log')
                left, right = [json.loads((folder / side / 'snapshot.json').read_text())
                               for side in ('koreader', 'kaia')]
                differences, images = compare(left, right, folder, settings['width'], settings['height'])
                report['cases'].append(dict(name=name, koreader_pages=left['page_count'], kaia_pages=right['page_count'],
                                            samples=len(images), differences=differences, images=images))
                line = (f"{'FAIL' if differences else 'PASS'} {name}: pages {left['page_count']}/{right['page_count']}, "
                        f"samples {len(images)}, differences {len(differences)}, "
                        f"max pixel delta {max((i['max_delta'] for i in images), default=0)}")
                print(line, flush=True)
                summary.append(line)
        report['passed'] = bool(report['cases']) and all(not c['differences'] for c in report['cases'])
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report['passed'] = False
        report['error'] = str(error)
        summary.append(f'ERROR: {error}; see per-process logs')
        print(summary[-1], flush=True)
    finally:
        report['seconds'] = round(time.monotonic() - start, 2)
        line = f"{'PASS' if report.get('passed') else 'FAIL'}: {len(report['cases'])} cases in {report['seconds']}s"
        print(line, flush=True)
        summary.append(line)
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        (output / 'summary.txt').write_text('\n'.join(summary) + '\n')
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--emulator', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    sys.exit(run(parser.parse_args()))
