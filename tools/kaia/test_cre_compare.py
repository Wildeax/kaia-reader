#!/usr/bin/env python3
"""Failure-path checks for the CREngine comparison gate."""
import copy
import contextlib
import io
import json
from pathlib import Path
import tempfile
import sys
from types import SimpleNamespace
import unittest

sys.dont_write_bytecode = True
from cre_compare import compare, image_diff, run


class ComparisonTests(unittest.TestCase):
    def test_reused_output_cannot_pass_with_stale_snapshots(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            previous = output / 'report.json'
            previous.write_text('{"passed": true}')
            with contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(run(SimpleNamespace(output=output, emulator=None)), 1)
            self.assertIn('output directory must be empty', errors.getvalue())
            self.assertEqual(previous.read_text(), '{"passed": true}')

    def test_each_exact_field_and_missing_sample_fails(self):
        snapshot = dict(page_count=3, pages=[dict(page=0, start='/a', end='/b', text='A\nB')])
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            for side in ('koreader', 'kaia'):
                (folder / side).mkdir()
                (folder / side / 'page-0.gray').write_bytes(bytes([0, 255]))
            for field in ('start', 'end', 'text'):
                with self.subTest(field=field):
                    changed = copy.deepcopy(snapshot)
                    changed['pages'][0][field] += ' '
                    differences, _ = compare(snapshot, changed, folder, 2, 1)
                    self.assertEqual(differences, [f'page 0: {field} differs'])
                    self.assertTrue((folder / f'page-0-{field}.diff').exists())
            changed = copy.deepcopy(snapshot)
            changed['page_count'] = 4
            self.assertTrue(compare(snapshot, changed, folder, 2, 1)[0])
            changed['pages'] = []
            self.assertIn('page 0: sample exists on only one side', compare(snapshot, changed, folder, 2, 1)[0])
            self.assertEqual(compare(snapshot, snapshot, folder, 2, 1)[0], [])
            self.assertIn('empty page samples', compare(changed, changed, folder, 2, 1)[0])

    def test_image_thresholds_reject_shifted_ink_and_accumulated_rounding(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            a, b, diff = [folder / name for name in ('a', 'b', 'diff.png')]
            a.write_bytes(bytes([0] * 100))
            b.write_bytes(bytes([1] + [0] * 99))
            self.assertTrue(image_diff(a, b, diff, 100, 1)['passed'])
            b.write_bytes(bytes([2] + [0] * 99))
            self.assertFalse(image_diff(a, b, diff, 100, 1)['passed'])
            b.write_bytes(bytes([1] * 100))
            self.assertFalse(image_diff(a, b, diff, 100, 1)['passed'])
            self.assertTrue(diff.read_bytes().startswith(b'\x89PNG'))
            b.write_bytes(b'')
            with self.assertRaises(ValueError):
                image_diff(a, b, diff, 100, 1)

    def test_manifest_is_nonempty_and_has_unique_files(self):
        manifest = json.loads((Path(__file__).parent / 'cre-corpus/manifest.json').read_text())
        files = [b['path'] for b in manifest['books']]
        self.assertTrue(files)
        self.assertEqual(len(files), len(set(files)))


if __name__ == '__main__':
    unittest.main()
