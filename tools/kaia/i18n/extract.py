#!/usr/bin/env python3
"""Extract literal tr, tr_ctx, and tr_n calls from Rust source."""
import ast
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[3]
CALL = re.compile(r'\b(tr|tr_ctx|tr_n)\s*\(')

def quoted(source, pos):
    if pos >= len(source) or source[pos] != '"':
        return None, pos
    end = pos + 1
    escaped = False
    while end < len(source):
        ch = source[end]
        if ch == '"' and not escaped:
            token = source[pos:end + 1]
            try:
                return ast.literal_eval(token), end + 1
            except (SyntaxError, ValueError):
                return None, end + 1
        if ch == "\\" and not escaped:
            escaped = True
        else:
            escaped = False
        end += 1
    return None, pos

def calls(text):
    for match in CALL.finditer(text):
        pos = match.end()
        args = []
        while pos < len(text):
            while pos < len(text) and text[pos].isspace():
                pos += 1
            value, pos = quoted(text, pos)
            if value is None:
                break
            args.append(value)
            while pos < len(text) and text[pos].isspace():
                pos += 1
            if pos < len(text) and text[pos] == ',':
                pos += 1
            else:
                break
        name = match.group(1)
        needed = {'tr': 1, 'tr_ctx': 2, 'tr_n': 2}[name]
        if len(args) >= needed:
            context = args[0] if name == 'tr_ctx' else None
            msgid = args[1] if name in ('tr_ctx', 'tr_n') else args[0]
            plural = args[2] if name == 'tr_n' and len(args) >= 3 else None
            yield context, msgid, plural, text.count('\n', 0, match.start()) + 1

def po_quote(value):
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n').replace('\t', '\\t').replace('\r', '\\r') + '"'

def main():
    collected = {}
    for path in sorted((ROOT / 'rust').rglob('*.rs')):
        try:
            source = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            continue
        for context, msgid, plural, line in calls(source):
            key = (context, msgid)
            item = collected.setdefault(key, {'plural': plural, 'refs': set()})
            if plural:
                item['plural'] = plural
            item['refs'].add(f'{path.relative_to(ROOT)}:{line}')
    out = ['msgid ""', 'msgstr ""', '"Content-Type: text/plain; charset=UTF-8\\n"', '"Content-Transfer-Encoding: 8bit\\n"', '']
    for (context, msgid), item in sorted(collected.items(), key=lambda pair: (pair[0][0] or '', pair[0][1])):
        out.append('#: ' + ' '.join(sorted(item['refs'])))
        if context is not None:
            out.append('msgctxt ' + po_quote(context))
        out.append('msgid ' + po_quote(msgid))
        if item['plural'] is not None:
            out.extend(['msgid_plural ' + po_quote(item['plural']), 'msgstr[0] ""', 'msgstr[1] ""'])
        else:
            out.append('msgstr ""')
        out.append('')
    target = ROOT / 'l10n-kaia/templates/kaia.pot'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text('\n'.join(out), encoding='utf-8')
    print(f'Wrote {target.relative_to(ROOT)} with {len(collected)} entries')

if __name__ == '__main__':
    main()
