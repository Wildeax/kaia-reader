#!/usr/bin/env python3
"""Extract the literal tr, tr_ctx and tr_n calls from the Rust sources.

The output is l10n-kaia/templates/kaia.pot. It holds only Kaia's own strings.
A string whose msgid (and msgctxt, for tr_ctx) is already in KOReader's template
l10n/templates/koreader.pot is left out: the KOReader catalogs translate it, so
Kaia reuses those translations. The script prints both counts.

The script exits with status 1 and writes nothing when a tr call has an argument
that is not a string literal, because such a string would be missing from the
template without a warning. Use `--reused` to list the reused msgids.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
TARGET = ROOT / 'l10n-kaia/templates/kaia.pot'
KOREADER_TEMPLATE = ROOT / 'l10n/templates/koreader.pot'

CALL = re.compile(r'(?<!\w)(tr|tr_ctx|tr_n)\s*\(')
# kaia-i18n defines the functions. Its own calls are not UI strings.
SKIP_CRATES = {'kaia-i18n'}
SKIP_DIRS = {'target', 'tests'}
ESCAPES = {'n': '\n', 'r': '\r', 't': '\t', '0': '\0', '\\': '\\', '"': '"', "'": "'"}
PO_ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '"': '"', '\\': '\\'}


def rust_string(text, pos):
    """Decode the Rust string literal that starts at text[pos] (a double quote).

    Returns (value, end) or (None, pos) when there is no literal at pos.
    Handles the escapes Rust accepts in plain string literals, including
    \\u{...} and line continuations.
    """
    if pos >= len(text) or text[pos] != '"':
        return None, pos
    out = []
    i = pos + 1
    while i < len(text):
        ch = text[i]
        if ch == '"':
            return ''.join(out), i + 1
        if ch != '\\':
            out.append(ch)
            i += 1
            continue
        i += 1
        if i >= len(text):
            break
        esc = text[i]
        if esc == 'u' and text[i + 1:i + 2] == '{':
            close = text.find('}', i + 2)
            if close < 0:
                return None, pos
            out.append(chr(int(text[i + 2:close].replace('_', ''), 16)))
            i = close + 1
        elif esc == 'x':
            out.append(chr(int(text[i + 1:i + 3], 16)))
            i += 3
        elif esc == '\n':
            i += 1
            while i < len(text) and text[i] in ' \t\r\n':
                i += 1
        elif esc in ESCAPES:
            out.append(ESCAPES[esc])
            i += 1
        else:
            return None, pos
    return None, pos


def tr_calls(text):
    """Yield (name, args, line) for each tr, tr_ctx and tr_n call in `text`.

    `args` holds the literal arguments that were read, in order. A call with
    fewer literals than it needs is yielded with the literals it did have.
    """
    for match in CALL.finditer(text):
        line_start = text.rfind('\n', 0, match.start()) + 1
        if '//' in text[line_start:match.start()]:
            continue
        if re.search(r'\bfn\s+$', text[max(0, match.start() - 16):match.start()]):
            continue
        name = match.group(1)
        needed = {'tr': 1, 'tr_ctx': 2, 'tr_n': 2}[name]
        args = []
        pos = match.end()
        while len(args) < needed:
            while pos < len(text) and text[pos].isspace():
                pos += 1
            value, end = rust_string(text, pos)
            if value is None:
                break
            args.append(value)
            pos = end
            while pos < len(text) and text[pos].isspace():
                pos += 1
            if pos < len(text) and text[pos] == ',':
                pos += 1
            else:
                break
        line = text.count('\n', 0, match.start()) + 1
        yield name, args, needed, line


def po_unquote(token):
    body = token[1:-1]
    return re.sub(r'\\(.)', lambda m: PO_ESCAPES.get(m.group(1), '\\' + m.group(1)), body)


def po_quote(value):
    escaped = value.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
    escaped = escaped.replace('\t', '\\t').replace('\r', '\\r')
    return '"' + escaped + '"'


def koreader_msgids():
    """The (msgctxt, msgid) pairs of KOReader's template."""
    if not KOREADER_TEMPLATE.is_file():
        sys.exit(f'missing {KOREADER_TEMPLATE.relative_to(ROOT)}: run `git submodule update --init l10n`')
    found = set()
    context = msgid = None
    field = None

    def finish():
        if msgid:
            found.add((context, msgid))

    for raw in KOREADER_TEMPLATE.read_text(encoding='utf-8').split('\n'):
        line = raw.strip()
        if not line:
            finish()
            context = msgid = field = None
            continue
        if line.startswith('#'):
            continue
        if line.startswith('"'):
            value = po_unquote(line)
            if field == 'ctx':
                context = (context or '') + value
            elif field == 'id':
                msgid = (msgid or '') + value
            continue
        key, _, rest = line.partition(' ')
        if key == 'msgctxt':
            field = 'ctx'
            context = po_unquote(rest)
        elif key == 'msgid':
            field = 'id'
            msgid = po_unquote(rest)
        else:
            field = None
    finish()
    return found


def main():
    reused_msgids = koreader_msgids()
    collected = {}
    problems = []
    for path in sorted((ROOT / 'rust').rglob('*.rs')):
        rel = path.relative_to(ROOT)
        if set(rel.parts) & SKIP_DIRS or (len(rel.parts) > 1 and rel.parts[1] in SKIP_CRATES):
            continue
        text = path.read_text(encoding='utf-8')
        for name, args, needed, line in tr_calls(text):
            where = f'{rel}:{line}'
            if len(args) < needed:
                problems.append(f'{where}: {name}() needs a string literal argument')
                continue
            if name == 'tr_ctx':
                context, msgid, plural = args[0], args[1], None
            elif name == 'tr_n':
                context, msgid, plural = None, args[0], args[1]
            else:
                context, msgid, plural = None, args[0], None
            entry = collected.setdefault((context, msgid), {'plural': None, 'refs': set()})
            if plural is not None:
                entry['plural'] = plural
            entry['refs'].add(where)
    if problems:
        print('Not written. These calls need a string literal:', file=sys.stderr)
        for problem in problems:
            print('  ' + problem, file=sys.stderr)
        sys.exit(1)

    own = {key: item for key, item in collected.items() if key not in reused_msgids}
    reused = sorted(key for key in collected if key in reused_msgids)
    print(f'{len(collected)} distinct strings in the Rust sources: '
          f'{len(own)} Kaia own (written to {TARGET.relative_to(ROOT)}), '
          f'{len(reused)} reuse KOReader msgids.')
    if '--reused' in sys.argv[1:]:
        for context, msgid in reused:
            print(f'reused: {context + " | " if context else ""}{msgid}')

    out = ['msgid ""', 'msgstr ""',
           '"Content-Type: text/plain; charset=UTF-8\\n"',
           '"Content-Transfer-Encoding: 8bit\\n"', '']
    for (context, msgid), item in sorted(own.items(), key=lambda pair: (pair[0][0] or '', pair[0][1])):
        out.append('#: ' + ' '.join(sorted(item['refs'])))
        if context is not None:
            out.append('msgctxt ' + po_quote(context))
        out.append('msgid ' + po_quote(msgid))
        if item['plural'] is not None:
            out.extend(['msgid_plural ' + po_quote(item['plural']), 'msgstr[0] ""', 'msgstr[1] ""'])
        else:
            out.append('msgstr ""')
        out.append('')
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    TARGET.write_text('\n'.join(out), encoding='utf-8')


if __name__ == '__main__':
    main()
