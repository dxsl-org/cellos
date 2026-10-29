"""S10-7.4 wiring of the retained S10-7.2 memory observer into the retained observer (read-only).

Where every range comes from (nothing is written to the guest; only RSP `p`/`m` reads and the
retained physical-memory view are used, exactly like `observer._snapshot` already does):

* ELF PT_LOAD ranges  - the retained observer instance (`inst['base'] + loads[i]['va']`, `memsz`).
* user stack          - the retained observer instance (`stack_bottom` .. `stack_top`): the entry stop
                        read `rsp`, rounded up to a page, mapped 16 pages and proved the two guard pages
                        below unmapped (retained `Observer._entry`).  Here the extent is re-derived for any
                        task by `discover_user_stack`.
* kernel stack        - read at the ENTRY STOP of the task (the task is the running one, user mode,
                        first instruction): kernel static `CPU_LOCALS[0].kernel_rsp` (= `kernel_trap_sp` =
                        kstack_top - 288, written by `set_kernel_stack` at every switch-in) cross-checked with
                        `TABLES[0].tss.rsp0`; `top = round_up_page(kernel_rsp)` with `top - kernel_rsp == 288`;
                        usable = the `pages` pages below `top`, 2 guard pages below them unmapped.  OWNERSHIP:
                        the spawn path seeded the 288-byte ViTrapFrame at `kernel_rsp` with `sepc = entry` (+264)
                        and `regs[2] = user stack top` (+16) (kernel/src/task.rs prime_user_mode_entry; `__trap_exit`
                        does not clear them), so the range is accepted only if `sepc == the entry pc` and
                        `regs[2] == the task's user stack top` (a stale/other task's stack cannot satisfy both).  Symbol
                        addresses come from the measurement kernel's own `.symtab` (non-PIE EXEC, no slide).
                        The value is only valid at that stop (later stops interrupt arbitrary tasks), so it is
                        stored in the instance record and reused at every snapshot.
* heap arena          - the cellbox ELF `.symtab` symbol `___CUSTOM_HEAP_ARENA` (6 MiB, inside .bss) at
                        `inst['base'] + st_value` (B2 shell: `ostd::heap::HEAP_MEM`, 1 MiB; residency only).
* pre-run prime proof - `verify_primed` at the same entry stop: user stack `[bottom, rsp & ~4095)`, kernel stack
                        `[usable_start, top - PAGE)`.  The page(s) holding the initial frame are excluded
                        because the loader/spawn path writes there before the first instruction.  If a byte
                        differs the proof is recorded as failed and `exact` stays false (honest flag, no STOP);
                        unreadable memory, undiscoverable or inconsistent ranges STOP the run (fail closed).
"""
import json
import struct
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402

for _p in (ENV.MEMORY, ENV.RUNTIME):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))
from guest_view import GuestMemoryError, GuestUnmappedError, GuestView, PAGE  # noqa: E402
from memory_snapshot import collect, regions_from_observer_instance  # noqa: E402
from stack_watermark import PrimeVerificationError, StackRangeError, verify_primed  # noqa: E402
from status_select import StatusError, select_status  # noqa: E402,F401  (re-exported for the surfaces)

TRAP_FRAME_BYTES = 288          # hal/arch/x86/src/x86_64/boot.rs: "ViTrapFrame (288 bytes, #[repr(C)])"
TRAP_SP_OFFSET = 16             # ViTrapFrame.regs[2] = user stack top (seeded by prime_user_mode_entry)
TRAP_IRET_OFFSET = 216          # __trap_exit builds the iretq frame (rip, cs, rflags, rsp, ss) in regs[27..31]
TRAP_SEPC_OFFSET = 264          # ViTrapFrame.sepc = entry point (user RIP)
GUARD_PAGES = 2                 # kernel/src/task/stack.rs STACK_GUARD_PAGES
CPU_LOCAL_BYTES = 64            # hal/arch/x86/src/x86_64/syscall.rs CpuLocal size
CPU_LOCALS_BYTES = 128          # MAX_CPUS = 2
TSS_RSP0_OFFSET = 64 + 4        # CpuTables { gdt: Gdt(8*8 bytes), tss: Tss { _r0: u32, rsp0: u64 } }
HEAP_ARENA_BYTES = 6 * 1024 * 1024
KERNEL_NEEDLES = {'cpu_locals': (b'7syscall', b'10CPU_LOCALS'), 'tss_tables': (b'3gdt', b'6TABLES')}


class DiscoveryError(Exception):
    """A stack/heap range could not be established from guest state or the observer: STOP (fail closed)."""


# ------------------------------------------------------------------ static inputs (host files)
class KernelSymbols:
    """Addresses of the per-CPU kernel records, read from the measurement kernel's .symtab."""

    def __init__(self, kernel_path):
        try:
            symbols = ENV.elf_symbols(kernel_path)
            cpu = ENV.unique_symbol(symbols, *KERNEL_NEEDLES['cpu_locals'], size=CPU_LOCALS_BYTES, what='CPU_LOCALS')
            tss = ENV.unique_symbol(symbols, *KERNEL_NEEDLES['tss_tables'], size=384, what='gdt TABLES')
        except ENV.EnvError as error:
            raise DiscoveryError(f'kernel per-CPU symbols not discoverable: {error}') from error
        self.cpu_locals, self.tables = cpu['value'], tss['value']
        if self.cpu_locals < 0xFFFF_8000_0000_0000 or self.tables < 0xFFFF_8000_0000_0000:
            raise DiscoveryError('kernel per-CPU symbols are not higher-half addresses (PIE/relocated kernel?)')


def heap_arena_symbol(elf_path, *, kind):
    """(relative VA, size) of the task's static heap arena from its own ELF symbol table."""
    try:
        symbols = ENV.elf_symbols(elf_path)
        if kind == 'cellbox':
            symbol = ENV.unique_symbol(symbols, b'7cellbox', b'19___CUSTOM_HEAP_ARENA', size=HEAP_ARENA_BYTES, what='cellbox heap arena')
        elif kind == 'b2-shell':
            symbol = ENV.unique_symbol(symbols, b'4ostd', b'4heap', b'8HEAP_MEM', size=1024 * 1024, what='B2 shell heap arena')
        else:
            raise DiscoveryError(f'unknown heap arena kind {kind!r}')
    except ENV.EnvError as error:
        raise DiscoveryError(f'heap arena not discoverable in {elf_path}: {error}') from error
    loads = ENV.elf_load_segments(elf_path)
    inside = any(seg['va'] <= symbol['value'] and symbol['value'] + symbol['size'] <= seg['va'] + seg['memsz'] for seg in loads)
    if not inside:
        raise DiscoveryError(f'heap arena {symbol["value"]:#x}+{symbol["size"]:#x} is outside every PT_LOAD segment')
    return symbol['value'], symbol['size']


# ------------------------------------------------------------------ discovery at a stop
def _mapped(view, va):
    return view.translate(va & ~(PAGE - 1))[0] is not None


def discover_user_stack(view, rsp, *, allowed_pages):
    """User stack extent from the entry-stop `rsp`: contiguous present run below top, 2 guard pages absent."""
    top = (rsp + PAGE - 1) & ~(PAGE - 1)
    run = 0
    while _mapped(view, top - (run + 1) * PAGE) and run <= max(allowed_pages) + 1:
        run += 1
    if run not in allowed_pages:
        raise DiscoveryError(f'user stack run below {top:#x} is {run} pages, expected one of {tuple(allowed_pages)}')
    bottom = top - run * PAGE
    for guard in range(1, GUARD_PAGES + 1):
        if _mapped(view, bottom - guard * PAGE):
            raise DiscoveryError(f'user stack guard page {bottom - guard * PAGE:#x} is mapped')
    return {'bottom': bottom, 'top': top, 'pages': run, 'source': 'entry-stop rsp page + present-page run + guard pages absent'}


def discover_kernel_stack(view, symbols, *, allowed_pages, entry_pc, user_top, read=None):
    """Kernel stack of the CURRENT (entry-stop) task from CPU_LOCALS[0].kernel_rsp, cross-checked with TSS.rsp0 and owned by
    the task through its seeded trap frame (sepc == entry_pc, regs[2] == user_top)."""
    read = read or view.read
    try:
        cpu = read(symbols.cpu_locals, CPU_LOCALS_BYTES)
        tss = read(symbols.tables + TSS_RSP0_OFFSET, 8)
    except GuestMemoryError as error:
        raise DiscoveryError(f'kernel per-CPU record unreadable: {type(error).__name__}: {error}') from error
    kernel_rsp, other_cpu = struct.unpack_from('<Q', cpu, 0)[0], struct.unpack_from('<Q', cpu, CPU_LOCAL_BYTES)[0]
    tss_rsp0 = struct.unpack('<Q', tss)[0]
    if kernel_rsp == 0 or other_cpu != 0:
        raise DiscoveryError(f'CPU_LOCALS kernel_rsp[0]={kernel_rsp:#x} kernel_rsp[1]={other_cpu:#x}: not a single online hart')
    if tss_rsp0 != kernel_rsp:
        raise DiscoveryError(f'TSS.rsp0 {tss_rsp0:#x} != CPU_LOCALS.kernel_rsp {kernel_rsp:#x}')
    top = (kernel_rsp + PAGE - 1) & ~(PAGE - 1)
    if top - kernel_rsp != TRAP_FRAME_BYTES:
        raise DiscoveryError(f'kernel_rsp {kernel_rsp:#x} is {top - kernel_rsp} bytes below its page top, expected {TRAP_FRAME_BYTES}')
    run = 0
    while _mapped(view, top - (run + 1) * PAGE) and run <= max(allowed_pages) + 1:
        run += 1
    if run not in allowed_pages:
        raise DiscoveryError(f'kernel stack run below {top:#x} is {run} pages, expected one of {tuple(allowed_pages)}')
    bottom = top - run * PAGE
    for guard in range(1, GUARD_PAGES + 1):
        if _mapped(view, bottom - guard * PAGE):
            raise DiscoveryError(f'kernel stack guard page {bottom - guard * PAGE:#x} is mapped')
    try:
        frame = read(kernel_rsp, TRAP_FRAME_BYTES)
    except GuestMemoryError as error:
        raise DiscoveryError(f'seeded trap frame unreadable at {kernel_rsp:#x}: {type(error).__name__}: {error}') from error
    sepc, seeded_sp = struct.unpack_from('<Q', frame, TRAP_SEPC_OFFSET)[0], struct.unpack_from('<Q', frame, TRAP_SP_OFFSET)[0]
    if sepc != entry_pc or seeded_sp != user_top:
        raise DiscoveryError(f'kernel stack at {kernel_rsp:#x} is not this task\'s: trap-frame sepc {sepc:#x} (entry {entry_pc:#x}), '
                             f'regs[2] {seeded_sp:#x} (user stack top {user_top:#x})')
    iret = struct.unpack_from('<5Q', frame, TRAP_IRET_OFFSET)      # built in place by __trap_exit: rip, cs, rflags, rsp, ss
    return {'bottom': bottom, 'top': top, 'pages': run, 'kernel_rsp': kernel_rsp, 'tss_rsp0': tss_rsp0,
            'owner_check': {'sepc': sepc, 'regs2': seeded_sp, 'iretq_rip': iret[0], 'iretq_cs': iret[1], 'iretq_rsp': iret[3], 'iretq_ss': iret[4],
                            'note': 'sepc/regs[2] are required; the iretq words are recorded evidence only'},
            'cpu_locals_va': symbols.cpu_locals, 'tss_va': symbols.tables + TSS_RSP0_OFFSET,
            'source': 'CPU_LOCALS[0].kernel_rsp == TSS.rsp0 at the entry stop; top-kernel_rsp == 288 (trap frame)'}


def prime_proof(view, start, limit):
    """verify_primed wrapper: failed proof is a recorded flag (exact stays false), unreadable memory is a STOP."""
    try:
        proof = verify_primed(view, start, limit)
        return proof, {'verified': True, 'start': start, 'limit': limit, 'bytes_checked': proof['bytes_checked']}
    except PrimeVerificationError as error:
        return None, {'verified': False, 'start': start, 'limit': limit, 'first_differing_va': error.va,
                      'byte': error.got, 'reason': str(error)}
    except (GuestMemoryError, StackRangeError) as error:
        raise DiscoveryError(f'pre-run prime proof [{start:#x},{limit:#x}) failed to run: {type(error).__name__}: {error}') from error


def entry_record(view, *, label, rsp, pc, user_pages, kernel_pages, symbols, user_stack=None):
    """Everything taken at an entry stop for one task: ranges, proofs and their honest status."""
    try:
        user = discover_user_stack(view, rsp, allowed_pages=user_pages)
        if user_stack is not None and (user_stack['bottom'], user_stack['top']) != (user['bottom'], user['top']):
            raise DiscoveryError(f'observer user stack [{user_stack["bottom"]:#x},{user_stack["top"]:#x}) != '
                                 f'independently derived [{user["bottom"]:#x},{user["top"]:#x})')
        kernel = discover_kernel_stack(view, symbols, allowed_pages=kernel_pages, entry_pc=pc, user_top=user['top'])
    except GuestMemoryError as error:
        raise DiscoveryError(f'guest memory unreadable while deriving the stack ranges: {type(error).__name__}: {error}') from error
    user_limit = rsp & ~(PAGE - 1)
    if user_limit <= user['bottom']:
        raise DiscoveryError(f'initial rsp {rsp:#x} leaves no unwritten user stack page to prove primed')
    user_proof, user_status = prime_proof(view, user['bottom'], user_limit)
    kernel_proof, kernel_status = prime_proof(view, kernel['bottom'], kernel['top'] - PAGE)
    return {
        'label': label, 'user_stack': user, 'kernel_stack': kernel, 'initial_rsp': rsp,
        'primed_proofs': {'user-stack': user_proof, 'kernel-stack': kernel_proof},
        'primed_status': {'user-stack': user_status, 'kernel-stack': kernel_status},
        'proof_scope': ('pre-run verify_primed taken at the ENTRY STOP (before the first user instruction); the top page of the '
                        'kernel stack (trap frame, switch frame) and the user page holding the initial rsp are written by the spawn '
                        'path before that stop and are excluded from the proof, so a depth below one page cannot be told from untouched.'),
        'ns': time.monotonic_ns(),
    }


# ------------------------------------------------------------------ observer wiring
class MemoryContext:
    """Per-run configuration of the memory wiring (all host-derived, all validated at construction)."""

    def __init__(self, *, provenance, kernel_path, heap_arena_rel, user_pages, kernel_pages):
        if 'test-hooks' not in provenance.get('kernel_features', ()):
            raise ValueError("provenance.kernel_features must contain 'test-hooks'")
        if not provenance.get('kernel_sha256'):
            raise ValueError('provenance must carry the measurement kernel sha256')
        actual = ENV.sha256_file(kernel_path)
        if actual != provenance['kernel_sha256']:
            raise ValueError(f'measurement kernel {kernel_path} sha256 {actual} != provenance {provenance["kernel_sha256"]}')
        if not ENV.kernel_has_test_hooks(kernel_path):
            raise ValueError('measurement kernel has no test_hook_prime_watermark symbol: not a test-hooks build')
        self.provenance = provenance
        self.symbols = KernelSymbols(kernel_path)
        self.heap_arena_rel = heap_arena_rel
        self.user_pages = tuple(user_pages)
        self.kernel_pages = tuple(kernel_pages)


class MemoryMixin:
    """Adds entry-stop discovery/proof and a memory record at every retained snapshot point.

    Place first in the MRO: `class MyObserver(MemoryMixin, <retained observer>)`; the constructor takes the extra
    keyword `memory=MemoryContext(...)`.  `SKIP_SNAPSHOT_NAMES` lists retained control requests that are not phases
    (the feed-hook disarm).
    """
    SKIP_SNAPSHOT_NAMES = ()

    def __init__(self, *args, memory, **kwargs):
        self.memory = memory
        self.memory_entries = {}
        self.memory_records = []
        super().__init__(*args, **kwargs)

    # entry stop -----------------------------------------------------------------
    def _entry(self, pc):
        before = set(self.instances)
        super()._entry(pc)
        if pc in self.instances and pc not in before:
            self._memory_entry(self.instances[pc])

    def _memory_entry(self, inst):
        view = GuestView(inst['cr3'], self.client.read_physical_memory)
        stack = {'bottom': inst['stack_bottom'], 'top': inst['stack_top'], 'pages': (inst['stack_top'] - inst['stack_bottom']) // PAGE,
                 'source': 'retained observer instance (entry-stop rsp page, 16 pages mapped, 2 guard pages verified unmapped)'}
        try:
            record = entry_record(view, label=f"{inst['role']}#{inst['id']}", rsp=inst['initial_rsp'], pc=inst['base'], user_pages=self.memory.user_pages,
                                  kernel_pages=self.memory.kernel_pages, symbols=self.memory.symbols, user_stack=stack)
        except DiscoveryError as error:
            self.memory_failure = str(error)
            raise self._stop(f'memory wiring cannot establish stack ranges for instance {inst["id"]}: {error}')
        record['instance'] = inst['id']
        record['role'] = inst['role']
        self.memory_entries[inst['base']] = record
        ENV.dump(self.m.work / f'evidence/memory-entry-{inst["id"]}.json', record)

    def _stop(self, message):
        from common import Stop  # retained exception type
        return Stop(message)

    # snapshot -------------------------------------------------------------------
    def _snapshot(self, name):
        result = super()._snapshot(name)
        if name not in self.SKIP_SNAPSHOT_NAMES:
            self._memory_snapshot(name)
        return result

    def _memory_snapshot(self, name):
        if self.focus is None or self.focus not in self.memory_entries:
            raise self._stop('memory snapshot: no focus instance with an entry-stop record')
        inst = self.instances[self.focus]
        entry = self.memory_entries[self.focus]
        view = GuestView(inst['cr3'], self.client.read_physical_memory)
        used = int.from_bytes(self.client.read_memory(inst['heap'], 8), 'little')
        arena = None
        if self.memory.heap_arena_rel is not None:
            arena = (inst['base'] + self.memory.heap_arena_rel[0], self.memory.heap_arena_rel[1])
        kernel = (entry['kernel_stack']['bottom'], entry['kernel_stack']['top'] - entry['kernel_stack']['bottom'])
        regions = regions_from_observer_instance(inst, self.loads, kernel_stack=kernel, heap_arena=arena, heap_used_bytes=used)
        record = collect(view, regions, phase=name, instance=inst['id'], provenance=self.memory.provenance,
                         primed_proofs=entry['primed_proofs'])
        record['entry'] = {k: entry[k] for k in ('label', 'initial_rsp', 'primed_status', 'proof_scope', 'role')}
        record['entry']['kernel_stack'] = entry['kernel_stack']
        record['entry']['user_stack'] = entry['user_stack']
        record['sp_not_used'] = 'the vCPU rsp at a pause belongs to an arbitrary task, not the focus instance; no sp cross-check is claimed'
        index = len(self.memory_records)
        self.memory_records.append({'phase': name, 'instance': inst['id'], 'ok': record['ok'], 'exact': record['all_stack_depths_exact'],
                                    'depth_bytes': record['stack_depth_bytes'], 'errors': record['errors']})
        ENV.dump(self.m.work / f'evidence/memory-{index:02d}-{name}.json', record)
        if record['errors']:
            raise self._stop(f'memory snapshot {name!r} failed closed: {record["errors"]!r}')
        return record

    def write_results(self):
        super().write_results()
        try:
            ENV.dump(self.m.work / 'evidence/memory-summary.json', {
                'schema': 'cellos.s10-7.memory-run-summary/1', 'provenance': self.memory.provenance,
                'entries': list(self.memory_entries.values()), 'snapshots': self.memory_records,
                'failure': getattr(self, 'memory_failure', None)})
        except Exception:  # noqa: BLE001 - result writing must not mask the observer error
            pass
