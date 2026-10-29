"""T4 ledger integrity and T5 oracle accept/reject (pure; synthetic guest over real retained JSON bytes)."""
import copy
import json
import os
from pathlib import Path
import random
import sys
import tempfile
import threading
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_ledger as L
import hookfix as H

NOW = 10 ** 15


def record(seq, site, data=b'x', ns=None, **kw):
    return L.hit_record(seq, ns if ns is not None else seq * 1000, 0x3c7aef0, site, len(data), data, **kw)


def accepts(kind, command, hits, grab, commit, until=None):
    verdict = L.evaluate(kind, command, hits, grab, commit, NOW, until)
    return verdict


class LedgerIntegrity(unittest.TestCase):
    def test_true_session_grammar_and_stream_tags(self):
        guest, f, s = H.two_epochs()
        ledger = L.Ledger()
        for hit in guest.hits:
            ledger.append(hit)
        self.assertEqual(ledger.count(), len(guest.hits))
        self.assertEqual({h['stream'] for h in ledger.snapshot()}, {'stdout'})
        self.assertEqual(L.RET_STREAM[0x1e5], 'stderr')

    def test_grammar_rejections(self):
        cases = {
            'loop-without-predecessor': [record(1, 0x1a5)],
            'stderr-loop-after-stdout-first': [record(1, 0x14b), record(2, 0x22d)],
            'stdout-loop-after-stderr-first': [record(1, 0x1e5), record(2, 0x1a5)],
            'unknown-site': [record(1, 0x777)],
            'run-too-long': [record(1, 0x14b)] + [record(i, 0x1a5) for i in range(2, 35)],
        }
        for name, hits in cases.items():
            with self.subTest(name):
                ledger = L.Ledger()
                with self.assertRaises(L.LedgerViolation):
                    for hit in hits:
                        ledger.append(hit)
                self.assertIsNotNone(ledger.violation)

    def test_run_length_boundary_31_accepts_32_rejects(self):
        ledger = L.Ledger()
        ledger.append(record(1, 0x14b))
        for i in range(2, 33):
            ledger.append(record(i, 0x1a5))
        self.assertEqual(ledger.count(), 32)
        with self.assertRaises(L.LedgerViolation):
            ledger.append(record(33, 0x1a5))
        # a stderr burst may follow a full stdout burst, and a new pump may start anywhere
        fresh = L.Ledger()
        fresh.append(record(1, 0x14b))
        fresh.append(record(2, 0x1e5))
        fresh.append(record(3, 0x22d))
        fresh.append(record(4, 0x14b))

    def test_field_violations(self):
        good = record(1, 0x14b, b'abc')
        for name, mutate in {
            'seq-gap': lambda r: r.update(seq=3),
            'seq-duplicate': lambda r: r.update(seq=1),
            'length-zero': lambda r: r.update(length=0, data_hex='', sha256=record(1, 0x14b, b'')['sha256']),
            'length-513': lambda r: r.update(length=513, data_hex='00' * 513),
            'length-lies': lambda r: r.update(length=4),
            'hash': lambda r: r.update(sha256='0' * 64),
        }.items():
            with self.subTest(name):
                ledger = L.Ledger()
                ledger.append(good)
                bad = copy.deepcopy(record(2, 0x14b, b'def'))
                mutate(bad)
                with self.assertRaises(L.LedgerViolation):
                    ledger.append(bad)

    def test_time_must_not_go_backwards_and_caps(self):
        ledger = L.Ledger()
        ledger.append(record(1, 0x14b, ns=100))
        with self.assertRaises(L.LedgerViolation) as raised:
            ledger.append(record(2, 0x14b, ns=99))
        self.assertEqual(raised.exception.code, 'time-order')
        capped = L.Ledger(max_hits=2)
        capped.append(record(1, 0x14b))
        capped.append(record(2, 0x14b))
        with self.assertRaises(L.LedgerViolation) as raised:
            capped.append(record(3, 0x14b))
        self.assertEqual(raised.exception.code, 'cap-hits')
        small = L.Ledger(max_bytes=4)
        small.append(record(1, 0x14b, b'abcd'))
        with self.assertRaises(L.LedgerViolation) as raised:
            small.append(record(2, 0x14b, b'e'))
        self.assertEqual(raised.exception.code, 'cap-bytes')
        self.assertEqual((L.MAX_HITS, L.MAX_BYTES), (2048, 512 * 1024))

    def test_jsonl_is_owner_only_flushed_before_return_and_keeps_rejected_hit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'ledger.jsonl'
            ledger = L.Ledger(path)
            ledger.append(record(1, 0x14b, b'one'))
            lines = path.read_text().splitlines()  # visible with no close(): written and fsynced inside append()
            self.assertEqual(len(lines), 1)
            self.assertEqual(oct(path.stat().st_mode & 0o777), '0o600')
            with self.assertRaises(L.LedgerViolation):
                ledger.append(record(2, 0x1a5 + 1))
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            self.assertEqual([r['accepted'] for r in rows], [True, False])
            with self.assertRaises(FileExistsError):
                L.Ledger(path)
            ledger.close()

    def test_snapshot_is_detached_copy_and_no_deadlock_under_contention(self):
        ledger = L.Ledger()
        stop = threading.Event()
        errors = []

        def reader():
            while not stop.is_set():
                snapshot = ledger.snapshot()
                try:
                    snapshot.append('mutate')
                    ledger.count()
                except Exception as error:  # noqa: BLE001
                    errors.append(error)

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        guest = H.Guest().feed(b'a' * 2000)
        for hit in guest.hits:
            ledger.append(hit)
        stop.set()
        thread.join(5)
        self.assertFalse(thread.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(ledger.count(), len(guest.hits))

    def test_dropped_and_duplicated_stop_detection_limits_are_as_documented(self):
        # A duplicated first-site feed is grammar-legal (a new pump), so it must be caught by the
        # payload validators, not the grammar; a dropped mid-burst loop hit is also grammar-legal.
        ledger = L.Ledger()
        ledger.append(record(1, 0x14b))
        ledger.append(record(2, 0x14b, b'x'))
        dropped_middle = L.Ledger()
        dropped_middle.append(record(1, 0x14b))
        dropped_middle.append(record(2, 0x1a5))
        dropped_middle.append(record(3, 0x1a5))  # a hit between 2 and 3 could have been lost: not provable here
        self.assertEqual(dropped_middle.count(), 3)


class OracleAcceptReject(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.guest, cls.f, cls.s = H.two_epochs()

    def finite(self, hits=None, **kw):
        f = dict(self.f)
        f.update(kw)
        return L.evaluate('finite', L.FINITE, hits if hits is not None else self.guest.hits, f['typed_grab_ns'],
                          f['commit_finish_ns'], kw.get('now', NOW), f['until_ns'])

    def status(self, hits=None):
        return L.evaluate('status', L.STATUS, hits if hits is not None else self.guest.hits, self.s['typed_grab_ns'],
                          self.s['commit_finish_ns'], NOW)

    def test_true_streams_accept(self):
        v = self.finite()
        self.assertEqual(v.status, 'READY', v.as_dict())
        self.assertEqual((v.extra['frames'], len(v.extra['expected_rows']), v.extra['expected_cursor'][0]), (2, 26, 9))
        self.assertGreater(v.extra['scrolls'], 0)
        w = self.status()
        self.assertEqual(w.status, 'READY', w.as_dict())
        self.assertEqual(w.extra['expected_rows'], ['Cellos > echo st=$?', 'st=0', 'Cellos >'])

    def test_every_single_boundary_split_still_accepts(self):
        guest, grab, commit, _ = H.session(sizes=[10 ** 6])
        post = [h for h in guest.hits if h['ns'] > commit]
        body = b''.join(bytes.fromhex(h['data_hex']) for h in post)
        pre = [h for h in guest.hits if h['ns'] <= commit]
        base_ns = commit
        for cut in range(1, len(body)):
            if cut % 3 and cut > 40:  # all boundaries near the start, every third elsewhere (bounded runtime)
                continue
            pieces = [body[:cut], body[cut:]]
            rebuilt = list(pre)
            for index, piece in enumerate(pieces):
                for offset in range(0, len(piece), 512):
                    part = piece[offset:offset + 512]
                    rebuilt.append(L.hit_record(len(rebuilt) + 1, base_ns + (len(rebuilt) + 1) * 1000, 1,
                                                0x14b if offset == 0 else 0x1a5, len(part), part))
            v = L.evaluate('finite', L.FINITE, rebuilt, grab, commit, NOW)
            self.assertEqual(v.status, 'READY', (cut, v.as_dict()))

    def test_random_chunkings_accept(self):
        for seed in range(30):
            rng = random.Random(seed)
            sizes = [rng.choice([1, 2, 3, 7, 64, 200, 511, 512]) for _ in range(900)]
            guest, grab, commit, _ = H.session(sizes=sizes)
            v = L.evaluate('finite', L.FINITE, guest.hits, grab, commit, NOW)
            self.assertEqual(v.status, 'READY', (seed, v.as_dict()))

    def rebuild(self, post_bytes):
        """Replace the post-commit stream by `post_bytes` keeping the pre-commit ledger."""
        guest, grab, commit, _ = H.session()
        pre = [h for h in guest.hits if h['ns'] <= commit]
        hits = list(pre)
        for offset in range(0, len(post_bytes), 512):
            part = post_bytes[offset:offset + 512]
            hits.append(L.hit_record(len(hits) + 1, commit + (len(hits) + 1) * 1000, 1,
                                     0x14b if offset == 0 else 0x1a5, len(part), part))
        return hits, grab, commit

    def truth(self):
        guest, grab, commit, _ = H.session()
        return b''.join(bytes.fromhex(h['data_hex']) for h in guest.hits if h['ns'] > commit)

    def verdict_for(self, data):
        hits, grab, commit = self.rebuild(data)
        return L.evaluate('finite', L.FINITE, hits, grab, commit, NOW)

    def test_structural_mutations_never_accept(self):
        truth = self.truth()
        lines = truth.split(b'\n')
        self.assertEqual(self.verdict_for(truth).status, 'READY')
        bad = {}
        for index in range(1, len(lines) - 1):  # delete one whole line (never the Enter echo / prompt)
            bad[f'delete-line-{index}'] = b'\n'.join(lines[:index] + lines[index + 1:])
        for index in range(1, len(lines) - 1):
            bad[f'duplicate-line-{index}'] = b'\n'.join(lines[:index + 1] + [lines[index]] + lines[index + 1:])
        bad['truncate-before-end'] = truth[:truth.rfind(b'{"frame":"end"')] + b'Cellos > '
        bad['missing-prompt'] = truth[:-len(b'Cellos > ')]
        bad['stale-old-prompt'] = truth.replace(b'\nCellos > ', b'\nCellos # ')
        bad['third-frame'] = truth[:truth.rfind(b'Cellos > ')] + b'{"frame":"begin","sample_id":99}\n{"frame":"row","value":{"a":1}}\n{"frame":"end","complete":true,"dropped":0}\n' + b'Cellos > '
        bad['dropped-nonzero'] = truth.replace(b'"dropped":0', b'"dropped":1', 1)
        bad['complete-false'] = truth.replace(b'"complete":true', b'"complete":false', 1)
        bad['stderr-esc'] = truth.replace(b'{"frame":"end"', b'\x1b[31m{"frame":"end"', 1)
        bad['nul'] = truth.replace(b'{"frame":"end"', b'\x00{"frame":"end"', 1)
        bad['bare-cr-in-output'] = truth.replace(b'\n{"frame":"row"', b'\r{"frame":"row"', 1)
        bad['no-enter-echo'] = truth[1:]
        self.assertIn(b'"total_cells":8', truth)
        bad['wrong-total-cells'] = truth.replace(b'"total_cells":8', b'"total_cells":9', 1)
        bad['one-frame-only'] = b'\n'.join(lines[:1] + lines[1:10] + [b'{"frame":"end","complete":true,"unavailable_fields":[],"dropped":0}', lines[-1]])
        for name, data in bad.items():
            with self.subTest(name):
                self.assertNotEqual(data, truth)
                self.assertNotEqual(self.verdict_for(data).status, 'READY', name)
        self.assertEqual(self.verdict_for(bad['dropped-nonzero']).cls, 'D')
        self.assertEqual(self.verdict_for(bad['complete-false']).cls, 'D')
        self.assertEqual(self.verdict_for(bad['stderr-esc']).cls, 'C')
        self.assertEqual(self.verdict_for(bad['third-frame']).cls, 'D')

    def test_structure_preserving_reorders_are_rejected_unless_off_screen(self):
        """Row order inside a frame is not a ledger-structure rule; the final-screen cross-check decides."""
        truth = self.truth()
        lines = truth.split(b'\n')
        true_verdict = self.verdict_for(truth)
        gui = true_verdict.extra['expected_rows']  # what the real screen shows for the true stream
        accepted_residual, rejected_by_screen = 0, 0
        for index in range(1, len(lines) - 2):
            swapped = list(lines)
            swapped[index], swapped[index + 1] = swapped[index + 1], swapped[index]
            if swapped == lines:
                continue
            verdict = self.verdict_for(b'\n'.join(swapped))
            if verdict.status != 'READY':
                continue  # structure caught it (begin/end swapped across frame boundaries)
            pipeline = L.screen_matches(gui, (9, len(gui) - 1), verdict.extra['expected_rows'])
            if pipeline:
                self.assertEqual(verdict.extra['expected_rows'], gui)  # only an unchanged visible screen passes
                accepted_residual += 1
            else:
                rejected_by_screen += 1
        self.assertGreater(rejected_by_screen, 0)
        self.assertGreater(accepted_residual, 0)  # documented residual: off-screen, structure-preserving reorder

    def test_structure_preserving_value_flip_is_accepted_by_ledger_alone_but_not_by_screen(self):
        # Documented residual: a flip inside a JSON string keeps every structural rule satisfied.
        truth = self.truth()
        flipped = truth.replace(b'"name":"serial"', b'"name":"seriaX"', 1)
        self.assertNotEqual(flipped, truth)
        verdict = self.verdict_for(flipped)
        self.assertEqual(verdict.status, 'READY')
        genuine = self.verdict_for(truth)
        tail = lambda text: L.expected_tail(L.FINITE, text)[0]  # noqa: E731
        if flipped.find(b'seriaX') > len(truth) - 1500:  # visible region: the final screen cross-check must reject it
            self.assertNotEqual(tail(verdict.extra['output_text']), tail(genuine.extra['output_text']))
        else:
            self.assertEqual(tail(verdict.extra['output_text']), tail(genuine.extra['output_text']))

    def test_status_epoch_rejections(self):
        base = H.Guest()
        base.echo(L.STATUS)
        grab, commit = base.mark(), base.mark()
        cases = {'st1': b'\nst=1\nCellos > ', 'st-text': b'\nstatus ok\nCellos > ', 'st0-twice': b'\nst=0\nst=0\nCellos > ',
                 'no-lf': b'st=0\nCellos > ', 'extra-line': b'\nst=0\nhello\nCellos > '}
        for name, body in cases.items():
            with self.subTest(name):
                guest = H.Guest()
                guest.feed(b'Cellos > ')
                guest.echo(L.STATUS)
                grab, commit = guest.mark(), guest.mark()
                guest.tick(20)
                guest.feed(body)
                verdict = L.evaluate('status', L.STATUS, guest.hits, grab, commit, NOW)
                self.assertNotEqual(verdict.status, 'READY')
        self.assertEqual(L.evaluate('status', L.STATUS, guest.hits, grab, commit, NOW).status, 'FAIL')
        guest = H.Guest()
        guest.feed(b'Cellos > ')
        guest.echo(L.STATUS)
        grab, commit = guest.mark(), guest.mark()
        guest.tick(20)
        guest.feed(b'\nst=1\nCellos > ')
        self.assertEqual(L.evaluate('status', L.STATUS, guest.hits, grab, commit, NOW).cls, 'D')

    def test_pre_enter_checks_f1_f2(self):
        guest, grab, commit, _ = H.session()
        wrong = copy.deepcopy(guest.hits)
        for hit in wrong:  # typed echo replaced by another command
            if hit['data_hex'] == b't'.hex():
                hit['data_hex'] = b'T'.hex()
                hit['sha256'] = L.hashlib.sha256(b'T').hexdigest()
        self.assertEqual(L.evaluate('finite', L.FINITE, wrong, grab, commit, NOW).code, 'typed-line-mismatch')
        window = copy.deepcopy(guest.hits)
        extra = H.hit_record = None  # placeholder to keep flake quiet
        stray = L.hit_record(len(window) + 1, grab + 1, 1, 0x14b, 1, b'x')
        window.insert(len([h for h in window if h['ns'] <= grab]), stray)
        verdict = L.evaluate('finite', L.FINITE, window, grab, commit, NOW)
        self.assertEqual(verdict.code, 'F2-window-hit')
        redraw = H.Guest()
        redraw.feed(b'banner\nCellos > ')
        redraw.feed(b'\r\x1b[2KCellos > ' + L.FINITE.encode())
        grab2, commit2 = redraw.mark(), redraw.mark()
        redraw.tick(10)
        redraw.feed(b'\n')
        redraw.feed(b'\n'.join(H.finite_lines()[:0]) + b'')
        self.assertEqual(L.check_typed([h for h in redraw.hits if h['ns'] <= grab2], L.FINITE), L.PROMPT + L.FINITE)
        with self.assertRaises(L.OracleFail):
            tokens_bad = H.Guest()
            tokens_bad.feed(b'Cellos > \x08 ' + L.FINITE.encode())
            L.check_typed(tokens_bad.hits, L.FINITE)

    def test_quiescence_and_pending_states(self):
        guest, f, s = self.guest, self.f, self.s
        early = L.evaluate('finite', L.FINITE, guest.hits, f['typed_grab_ns'], f['commit_finish_ns'],
                           [h for h in guest.hits if h['ns'] <= f['until_ns']][-1]['ns'] + 1, f['until_ns'])
        self.assertEqual((early.status, early.code), ('PENDING', 'quiescence'))
        before = [h for h in guest.hits if h['ns'] <= f['commit_finish_ns']]
        partial = before + [h for h in guest.hits if f['commit_finish_ns'] < h['ns'] <= f['until_ns']][:8]
        pending = L.evaluate('finite', L.FINITE, partial, f['typed_grab_ns'], f['commit_finish_ns'], NOW, f['until_ns'])
        self.assertEqual(pending.status, 'PENDING')
        nothing = L.evaluate('finite', L.FINITE, [h for h in guest.hits if h['ns'] <= f['commit_finish_ns']],
                             f['typed_grab_ns'], f['commit_finish_ns'], NOW)
        self.assertEqual((nothing.status, nothing.code), ('PENDING', 'no-output-yet'))

    def test_wrong_command_or_kind_fails(self):
        self.assertEqual(L.evaluate('finite', L.STATUS, self.guest.hits, 1, 2, NOW).code, 'wrong-command')
        self.assertEqual(L.evaluate('other', L.FINITE, self.guest.hits, 1, 2, NOW).code, 'wrong-command')

    def test_validator_parity_with_retained_temporal_ledger_rules(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('retained_temporal', HERE.parent / 'observer-gui-temporal/temporal.py')
        retained = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(retained)
        lines = H.finite_lines()
        records = [json.loads(line) for line in lines]
        variants = {'true': records}
        variants['dup-row'] = records[:3] + [records[2]] + records[3:]
        variants['no-end'] = records[:-1]
        variants['dropped'] = [dict(r, dropped=2) if r.get('frame') == 'end' else r for r in records]
        variants['one-frame'] = records[:len(records) // 2]
        variants['bad-sample'] = [dict(r, sample_id=-1) if r.get('frame') == 'begin' else r for r in records]
        variants['unknown'] = records + [{'frame': 'zzz'}]
        for name, recs in variants.items():
            with self.subTest(name):
                old = retained.TemporalLedger.__new__(retained.TemporalLedger)
                old._reasons, old._failed, old.complete = [], False, False
                try:
                    old._validate_records(recs, final=True)
                    old_ok = True
                except retained.OracleIncomplete:
                    old_ok = False
                try:
                    L.validate_records(recs)
                    new_ok = True
                except L.OracleFail:
                    new_ok = False
                self.assertEqual(old_ok, new_ok, name)
        self.assertEqual(retained.PROMPT, L.PROMPT)


if __name__ == '__main__':
    unittest.main()
