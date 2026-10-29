"""Offline synthetic exact-pixel temporal proof; never a historical desktop claim."""
import hashlib
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

from PIL import ImageDraw
import gui_temporal as G
from temporal import TemporalLedger, OracleIncomplete
from test_temporal import enter_commit, retained_lines, COLS, ROWS, PROMPT, COMMAND, STATUS_COMMAND

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('retained_grid_model', HERE.parent / 'observer-finite-oracle-analysis/prove_scroll.py')
PRIOR = importlib.util.module_from_spec(spec)
spec.loader.exec_module(PRIOR)


class SourceGrid(PRIOR.TranscriptGrid):
    def capture(self):
        return self.visible(), (self.x, self.y)


def prove():
    D = G.X.D
    glyph = G.M.GLYPH
    assert hashlib.sha256(glyph.read_bytes()).hexdigest() == G.M.GLYPH_SHA
    evidence = HERE / 'rendered-proof-final'
    evidence.mkdir(mode=0o700, exist_ok=False)
    work = evidence / 'work'
    (work / 'scratch').mkdir(parents=True)
    desk = object.__new__(D.Desk)
    D.Proof.__init__(desk, SimpleNamespace(work=str(work), evidence=str(evidence), glyph_bin=str(glyph)))
    desk.model = ['', D.BANNER, D.PROMPT]
    desk.window = lambda image: ((0, 0), image)
    grid = SourceGrid()
    grid.feed('\n' + D.BANNER + '\n' + PROMPT + COMMAND)
    samples = []
    def exact(label):
        expected_rows, expected_cursor = grid.capture()
        image = desk.render(expected_rows).copy()
        x = D.gui.PAD_X + grid.x * D.CELL_W
        y = D.gui.PAD_Y + grid.y * D.CELL_H + 2
        ImageDraw.Draw(image).rectangle((x, y, x + D.CELL_W - 1, y + D.CELL_H - 5), fill=(48, 220, 200))
        path = evidence / f'{label}.png'
        image.save(path)
        read = desk.read_screen(image)
        assert read == (expected_rows, expected_cursor), f'exact pixel read missing/mismatched {label}'
        sample = {'label': label, 'image': str(path.relative_to(HERE)), 'rows': read[0], 'cursor': read[1],
                  'exact_pixel_reader_accepts': True, 'scrolls': grid.scrolls}
        samples.append(sample)
        return read
    typed = exact('00-typed-original')
    finite = TemporalLedger(COLS, ROWS, *typed, COMMAND)
    finite.arm(enter_commit())
    grid.feed('\n')
    observations = []
    for index, line in enumerate(retained_lines()):
        grid.feed(line + '\n')
        read = exact(f'{index + 1:02d}-record')
        accepted = finite.observe(*read, 2_000_000_000 + index)
        assert not accepted, 'output without subsequent prompt cannot complete'
        observations.append(read)
    grid.feed(PROMPT)
    final = exact('21-fresh-prompt-echo-offscreen')
    complete = finite.observe(*final, 2_000_000_100)
    assert complete and PROMPT + COMMAND not in final[0]
    failures = []
    for label, trace in [('coalesced-final-only', [final]), ('dropped-middle', [observations[0], final])]:
        ledger = TemporalLedger(COLS, ROWS, *typed, COMMAND)
        ledger.arm(enter_commit())
        outcomes = []
        try:
            for index, read in enumerate(trace):
                outcomes.append(ledger.observe(*read, 2_000_000_000 + index))
        except OracleIncomplete as error:
            outcomes.append(f'{type(error).__name__}: {error}')
        assert not ledger.snapshot()['complete'] and True not in outcomes
        failures.append({'case': label, 'outcomes': outcomes, 'ledger': ledger.snapshot()})
    grid.feed(STATUS_COMMAND)
    status_typed = exact('22-status-typed')
    status = TemporalLedger(COLS, ROWS, *status_typed, STATUS_COMMAND, kind='status', expected_status=0)
    status.arm(enter_commit(18, finish=3.0))
    grid.feed('\nst=0\n' + PROMPT)
    status_final = exact('23-status-zero-newprompt')
    assert status.observe(*status_final, 3_000_000_001)
    report = {'scope': 'OFFLINE SYNTHETIC real retained renderer+actual headless JSON fixture, not missing desktop output',
              'complete_ordered_trace_accepts': bool(complete), 'coalesced_or_dropped_trace_rejects': True,
              'all_exact_pixel_samples_read': True, 'final_echo_offscreen': PROMPT + COMMAND not in final[0],
              'status_zero_own_epoch_accepts': status.snapshot()['complete'],
              'historical_desktop_completion_proved': False, 'candidate_runs': 0,
              'geometry': {'columns': COLS, 'rows': ROWS, 'output_physical_rows': 62},
              'glyph_sha256': G.M.GLYPH_SHA,
              'fixture': str(HERE.parent / 'observer-recurring/headless/runtime-result.json'),
              'fixture_sha256': G.X.sha(HERE.parent / 'observer-recurring/headless/runtime-result.json'),
              'samples': samples, 'complete_ledger': finite.snapshot(), 'status_ledger': status.snapshot(),
              'missed_output_failures': failures,
              'sampling_limit': 'Complete overlapping sequence proved only. Sparse actual frames may miss62row output and MUST STOP observability-incomplete, never inferred from command/status.'}
    G.X.dump(HERE / 'rendered-temporal-proof.json', report)
    return report
