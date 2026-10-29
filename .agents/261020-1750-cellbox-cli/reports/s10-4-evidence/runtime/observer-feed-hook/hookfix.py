"""Shared offline fixtures for the S10-4.17 tests (synthetic guest model over real retained bytes)."""
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_ledger as HL

RETAINED = HERE.parent / 'observer-recurring/headless/runtime-result.json'
S = 1_000_000_000


def finite_lines():
    """The unchanged actual two-frame `top --records ... | to jsonl | cat` rows retained from the headless run."""
    evidence = json.loads(RETAINED.read_text())
    event = next(item for item in evidence['surface_events'] if item.get('name') == 'finite-stream')
    assert event['command'] == HL.FINITE
    return list(event['rows'])


class Guest:
    """Deterministic Terminal-side model: every `feed` call becomes one hit with drain's return sites."""

    def __init__(self, start_ns=10 * S, rsp=0x3c7aef0):
        self.hits = []
        self.ns = start_ns
        self.rsp = rsp
        self.burst = None

    def tick(self, ms=7):
        self.ns += ms * 1_000_000

    def feed(self, data, stream='stdout', sizes=None, tick_ms=7):
        """One pump's drain of one stream: first feed at the first site, then loop site, <=32 feeds."""
        data = bytes(data)
        pos, count = 0, 0
        first, loop = (0x14b, 0x1a5) if stream == 'stdout' else (0x1e5, 0x22d)
        sizes = list(sizes or [])
        while pos < len(data):
            take = sizes.pop(0) if sizes else 512
            chunk = data[pos:pos + take]
            site = first if count == 0 else loop
            if count >= 32:
                self.burst = None
                count = 0
                site = first
            self.tick(tick_ms)
            self.hits.append(HL.hit_record(len(self.hits) + 1, self.ns, self.rsp, site, len(chunk), chunk,
                                           cr3=0x59000, rdi=0x3c60000, rsi=0x3c60100))
            pos += len(chunk)
            count += 1
        return self

    def echo(self, text):
        for char in text.encode():
            self.feed(bytes([char]))

    def mark(self):
        self.tick(300)
        return self.ns

    def clone_hits(self):
        return json.loads(json.dumps(self.hits))


def session(command=HL.FINITE, kind='finite', output_lines=None, sizes=None, status_after=True):
    """Boot banner -> typed finite command -> Enter -> output -> prompt [-> status epoch]."""
    guest = Guest()
    guest.feed("=== Cellos shell ready \u2014 type 'help' for commands ===\nCellos > ".encode())
    guest.echo(command)
    grab = guest.mark()
    enter_start = guest.mark()
    commit = guest.mark()
    guest.tick(50)
    guest.feed(b'\n')
    lines = finite_lines() if output_lines is None else output_lines
    body = ''.join(line + '\n' for line in lines).encode() if kind == 'finite' else b'st=0\n'
    guest.feed(body, sizes=sizes)
    guest.feed(b'Cellos > ')
    return guest, grab, commit, enter_start


def two_epochs():
    """Finite epoch then status epoch on one Guest; returns hits and both cut times."""
    guest, grab, commit, _ = session()
    f = {'typed_grab_ns': grab, 'commit_finish_ns': commit, 'until_ns': guest.ns}
    guest.tick(4000)
    guest.echo(HL.STATUS)
    grab2 = guest.mark()
    guest.mark()
    commit2 = guest.mark()
    guest.tick(50)
    guest.feed(b'\n')
    guest.feed(b'st=0\n')
    guest.feed(b'Cellos > ')
    s = {'typed_grab_ns': grab2, 'commit_finish_ns': commit2}
    return guest, f, s


def hits_hash(hits):
    return hashlib.sha256(b''.join(bytes.fromhex(h['data_hex']) for h in hits)).hexdigest()
