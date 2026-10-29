"""S10-7.4 measurement machine and surfaces (status selection); retained classes subclassed, never edited.

`MeasurementMachine` replaces `common.Machine.__init__`/`stop` (which verify the vanished
`/tmp/cellbox-s10-3d-resume/cand` tree) with verification against the measurement set's manifest.
The QEMU argv is the retained `D.Machine` argv unchanged (q35, TCG, 256M, user netdev `restrict=on`
without forwards, ISO cdrom, one NVMe data disk, no -smp, `-display none`, private serial/QMP/gdb unix
sockets) except that b2-headless drops the NVMe data disk (the B2 shared subset needs none).
"""
import argparse
import json
import re
import subprocess
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_memory as MEM  # noqa: E402  (also puts runtime/ on sys.path)

import common as X  # noqa: E402  (retained runtime/common.py)
D = X.D


def measurement_inputs(spec):
    """`machine.inputs` for the retained observer: ELF files from candidate.json (cache copy) + the set's ISO."""
    inputs = {'iso': {'path': str(spec.iso), 'sha256': ENV.expected_hashes(spec.name)['iso']}}
    if spec.kind == 'candidate':
        _record, files = ENV.candidate_files(spec.profile)
        inputs['files'] = {name: {'file': item['file'], 'sha256': item['sha256']}
                           for name, item in files.items() if name in ('/bin/cellbox', '/bin/terminal', '/bin/vfs')}
        for name, item in inputs['files'].items():
            X.need(ENV.sha256_file(item['file']) == item['sha256'], f'candidate file changed: {name}')
    else:
        inputs['files'] = {}
    return inputs


def drop_data_disk(argv):
    """argv without the retained `-drive ...id=p10fx` / `-device nvme,drive=p10fx` pair (exactly those four tokens)."""
    out, skip = [], 0
    for index, item in enumerate(argv):
        if skip:
            skip -= 1
            continue
        if item == '-drive' and 'id=p10fx' in argv[index + 1]:
            skip = 1
            continue
        if item == '-device' and argv[index + 1].startswith('nvme,drive=p10fx'):
            skip = 1
            continue
        out.append(item)
    X.need(len(out) == len(argv) - 4, 'data disk drop did not remove exactly -drive/-device')
    return out


def dry_argv(spec, scratch, *, observed, with_data_disk=True):
    """The exact argv a run would use, built in `scratch` without verifying/copying anything and without starting QEMU."""
    scratch = Path(scratch)
    (scratch / 'evidence').mkdir(parents=True, exist_ok=True)
    (scratch / 'sock').mkdir(mode=0o700, exist_ok=True)
    args = argparse.Namespace(profile=spec.profile, work=str(scratch), evidence=str(scratch / 'evidence'), iso=str(spec.iso),
                              accel='tcg', window=ENV.DEADLINES['total_window_s'],
                              glyph_bin=str(spec.glyph_bin) if spec.profile == 'desktop' else '')
    machine = D.Machine.__new__(D.Machine)
    D.Machine.__init__(machine, args, 'measurement', str(scratch / 'disk.img'), False)
    argv = drop_data_disk(machine.argv) if not with_data_disk else list(machine.argv)
    if observed:
        argv += ['-chardev', f'socket,path={scratch / "sock/gdb.sock"},server=on,wait=off,id=gdb0', '-gdb', 'chardev:gdb0', '-S']
    return argv


class MeasurementMachine(X.Machine):
    def __init__(self, spec, work, *, observed, with_data_disk=True):
        self.spec = spec
        self.work = Path(work)
        self.work.mkdir(mode=0o700, parents=True, exist_ok=False)
        for name in ('evidence', 'scratch', 'sock'):
            (self.work / name).mkdir(mode=0o700)
        check = ENV.verify_set_files(spec.name)
        X.need(check['all_match'], f'measurement set files differ from the manifest: {check["match"]}')
        X.need(ENV.kernel_has_test_hooks(spec.kernel), 'measurement kernel is not a test-hooks build')
        self.inputs = measurement_inputs(spec)
        self.source_disk_sha = None
        disk = self.work / 'disk.img'
        if with_data_disk:
            X.need(spec.fixture_disk.is_file(), f'run-inputs fixture disk missing: {spec.fixture_disk} (run prepare first)')
            subprocess.run(['cp', '--reflink=auto', '--sparse=always', str(spec.fixture_disk), str(disk)], check=True)
            self.source_disk_sha = ENV.sha256_file(spec.fixture_disk)
            X.need(ENV.sha256_file(disk) == self.source_disk_sha, 'fixture clone differs')
        args = argparse.Namespace(profile=spec.profile, work=str(work), evidence=str(self.work / 'evidence'),
                                  iso=str(spec.iso), accel='tcg', window=ENV.DEADLINES['total_window_s'],
                                  glyph_bin=str(spec.glyph_bin) if spec.profile == 'desktop' else '')
        D.Machine.__init__(self, args, 'measurement', str(disk), False)
        if not with_data_disk:
            self._drop_data_disk()
        self.observed = observed
        self.start_ns = None
        self.started = False
        if observed:
            self.gdb_path = self.work / 'sock/gdb.sock'
            self.argv += ['-chardev', f'socket,path={self.gdb_path},server=on,wait=off,id=gdb0', '-gdb', 'chardev:gdb0', '-S']
        self.provenance = ENV.provenance(spec.name)
        ENV.dump(self.work / 'evidence/input-provenance.json', {
            'set': spec.name, 'kernel_provenance': self.provenance, 'profile': spec.profile, 'iso': self.inputs['iso'],
            'argv': self.argv, 'fixture_disk': str(spec.fixture_disk) if with_data_disk else None,
            'fixture_disk_sha256': self.source_disk_sha, 'glyph_sha256': ENV.sha256_file(spec.glyph_bin) if spec.profile == 'desktop' else None,
            'mode': 'observed' if observed else 'untraced', 'set_files': check})

    def _drop_data_disk(self):
        self.argv = drop_data_disk(self.argv)
        (Path(self.a.evidence) / f'qemu-argv-{self.label}.txt').write_text(' '.join(self.argv).replace(self.a.work, '$WORK') + '\n')

    def stop(self):
        raw = D.Machine.stop(self) if self.started else b''
        if self.started:
            X.dump(self.work / 'evidence/serial-receipts.json', self.serial.receipts)
            self.serial.conn.close()
            self.err.close()
            self.qmp.file.close()
            self.qmp.sock.close()
        now = ENV.verify_set_files(self.spec.name)
        proof = {'set': self.spec.name, 'files_match_manifest': now['all_match'], 'actual': now['actual']}
        if self.source_disk_sha is not None:
            proof['source_disk_unchanged'] = ENV.sha256_file(self.spec.fixture_disk) == self.source_disk_sha
        X.dump(self.work / 'evidence/final-input-proof.json', proof)
        X.need(proof['files_match_manifest'] and proof.get('source_disk_unchanged', True), 'measurement input changed')
        return raw


# ------------------------------------------------------------------ status selection (S10-7.3)
def select_status_rows(rows):
    """Fail-closed selection of THIS command's status row; converts StatusError to the retained Stop."""
    try:
        return MEM.select_status(rows)
    except MEM.StatusError as error:
        raise X.Stop(f'status selection failed closed: {error}') from error


class DesktopStatusMixin:
    """Desktop `Surface.status()` replacement: the command returns the whole-screen rows, which may hold older st= rows."""

    def status(self):
        return select_status_rows(self.command('status', ENV.STATUS_CMD, ENV.DEADLINES['status_s']))


class HeadlessStatusMixin:
    """Headless `Surface.status()` replacement.

    `Surface.command` drops the echoed command line, so it cannot feed `select_status`.  The echo is re-verified with the
    retained `exact_echo_output` (UART echo reassembled across service/kernel lines must equal the typed command) and
    prepended to the rows that followed it, which is the screen/row shape the selector expects.
    """

    def status(self):
        timeout = ENV.DEADLINES['status_s']
        self.check()
        start = time.monotonic_ns()
        before, at = self.sh.submit(ENV.STATUS_CMD)
        X.need(self.sh.wait_prompt(before + 1, timeout), f'no new prompt after {ENV.STATUS_CMD!r} within {timeout}s')
        lines = D.logical_lines(D.strip_ansi(self.sh.raw()[at:]))
        output = X.exact_echo_output(lines, ENV.STATUS_CMD)
        rows = [ENV.STATUS_CMD] + [part for line in output for part in line.split('\n')]
        end = time.monotonic_ns()
        self.check()
        self.events.append({'command': ENV.STATUS_CMD, 'name': 'status', 'start_ns': start, 'end_ns': end, 'rows': rows})
        return select_status_rows(rows)


def surface_classes(hook_surface_cls):
    """(desktop, headless) measured surface classes; hook_surface_cls is `hook_surface.HookSurface`."""
    class MeasuredDesktopSurface(DesktopStatusMixin, hook_surface_cls):
        pass

    class MeasuredHeadlessSurface(HeadlessStatusMixin, X.Surface):
        pass
    return MeasuredDesktopSurface, MeasuredHeadlessSurface
