"""S10-4.17 T1: re-derive the static Terminal ELF facts the feed hook depends on.

Read-only: parses the immutable candidate Terminal ELF and runs objdump/nm/readelf on it.
Nothing here executes, patches or writes the ELF.  `derive()` returns a JSON-compatible
dict; `main()` archives it (with the raw disassembly fixtures and command receipts).
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
CANDIDATE = Path('/tmp/cellbox-s10-3d-resume/cand/candidate.json')
TERMINAL_SHA = '816718a20188bb797a65622d17087010e8d41c7a933248dc303847c7d11ff3cc'
CELLBOX_SHA = '8ea134442bb27719cf4c2ce5a8f3613f5667f88f1762a1d731cba9fd83ab54a2'
FEED_SYMBOL = '<terminal::sink::ByteSink>::feed'
DRAIN_SYMBOL = 'terminal::session::drain::<terminal_sdk::host::FrontendHost>'
FEED_VA = 0x1da80
RET_SITES = {0x14b: 'first', 0x1a5: 'first', 0x1e5: 'second', 0x22d: 'second'}
CALL_SITES = (0x146, 0x1a0, 0x1e0, 0x228)


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def candidate_elf(name):
    record = json.loads(CANDIDATE.read_text())['profiles']['desktop']['files'][name]
    path = Path(record['file'])
    if sha(path) != record['sha256']:
        raise AssertionError(f'{name} bytes differ from the qualified candidate record')
    return path, record['sha256']


def run(argv):
    done = subprocess.run(argv, capture_output=True, text=True, check=True)
    return done.stdout


def parse_elf(data):
    if data[:6] != b'\x7fELF\x02\x01':
        raise AssertionError('not ELF64LE')
    etype = struct.unpack_from('<H', data, 16)[0]
    phoff = struct.unpack_from('<Q', data, 32)[0]
    phent, phnum = struct.unpack_from('<HH', data, 54)
    shoff = struct.unpack_from('<Q', data, 40)[0]
    shent, shnum, shstr = struct.unpack_from('<HHH', data, 58)
    loads = []
    for i in range(phnum):
        kind, flags, off, va, _pa, filesz, memsz, _al = struct.unpack_from('<IIQQQQQQ', data, phoff + i * phent)
        if kind == 1:
            loads.append({'va': va, 'offset': off, 'filesz': filesz, 'memsz': memsz, 'flags': flags})
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * shent) for i in range(shnum)]
    names = data[sections[shstr][4]:sections[shstr][4] + sections[shstr][5]]
    byname = {names[s[0]:].split(b'\0', 1)[0].decode(): s for s in sections}
    return etype, loads, byname


def va_to_offset(loads, va):
    for load in loads:
        if load['va'] <= va < load['va'] + load['filesz']:
            return load['offset'] + va - load['va']
    raise AssertionError(f'va {va:#x} outside file-backed loads')


def relocs(data, byname):
    section = byname['.rela.dyn']
    offset, size = section[4], section[5]
    result = []
    for at in range(offset, offset + size, 24):
        r_offset, r_info, r_addend = struct.unpack_from('<QQq', data, at)
        result.append((r_offset, r_info & 0xffffffff, r_addend))
    return result


def symbols(path):
    table = {}
    for line in run(['nm', '-C', '-n', '-S', str(path)]).splitlines():
        match = re.match(r'([0-9a-f]+) (?:([0-9a-f]+) )?(\w) (.*)', line)
        if match:
            table[match[4]] = (int(match[1], 16), int(match[2], 16) if match[2] else None, match[3])
    return table


INSN = re.compile(r'\s*([0-9a-f]+):\t(?:[0-9a-f]{2} )+\s*(?:\t(.*))?$')


def parse_asm(text):
    """List of (address, mnemonic+operands) from objdump -d output."""
    rows = []
    for line in text.splitlines():
        match = INSN.match(line)
        if match and match[2]:
            rows.append((int(match[1], 16), match[2].split('#')[0].strip()))
    return rows


def dest_register(operands):
    return operands.split(',')[-1].strip() if ',' in operands else ''


RCX = {'%rcx', '%ecx', '%cx', '%cl', '%ch'}


def derive(write=True):
    terminal, terminal_hash = candidate_elf('/bin/terminal')
    cellbox, cellbox_hash = candidate_elf('/bin/cellbox')
    if terminal_hash != TERMINAL_SHA or cellbox_hash != CELLBOX_SHA:
        raise AssertionError('candidate ELF identity changed')
    data = terminal.read_bytes()
    etype, loads, byname = parse_elf(data)
    syms = symbols(terminal)
    feed_va, feed_size, feed_kind = syms[FEED_SYMBOL]
    drain_va, drain_size, _ = syms[DRAIN_SYMBOL]
    ordered = sorted(address for address, _size, _kind in syms.values())
    feed_end = next(address for address in ordered if address > feed_va)
    drain_end = next(address for address in ordered if address > drain_va)
    receipts = []

    def objdump(start, stop, name):
        argv = ['objdump', '-d', '-C', f'--start-address={start:#x}', f'--stop-address={stop:#x}', str(terminal)]
        text = run(argv)
        if write:
            (HERE / 'fixtures' / name).write_text(text)
        receipts.append({'argv': argv, 'stdout_fixture': name, 'sha256': hashlib.sha256(text.encode()).hexdigest()})
        return text

    feed_text = objdump(feed_va, feed_end, 'terminal-feed-full.asm')
    drain_text = objdump(drain_va, drain_end, 'terminal-drain-full.asm')
    whole_argv = ['objdump', '-d', '-C', str(terminal)]
    whole = run(whole_argv)
    receipts.append({'argv': whole_argv, 'stdout_fixture': None, 'sha256': hashlib.sha256(whole.encode()).hexdigest(),
                     'note': 'whole-ELF scan; text not archived (large), derived counts below'})
    feed = parse_asm(feed_text)
    drain = parse_asm(drain_text)

    # Every direct branch/call or rip-relative reference to the hook address, whole ELF.
    callers = []
    for line in whole.splitlines():
        match = re.match(rf'\s*([0-9a-f]+):\t.*\t(call|jmp|j[a-z]+)\s+{feed_va:x} ', line)
        if match:
            callers.append((int(match[1], 16), match[2]))
    pointer_refs = [line.strip() for line in whole.splitlines() if re.search(rf'#\s*{feed_va:x}\b', line)]
    rela = relocs(data, byname)
    reloc_hits = [r for r in rela if r[2] == feed_va or feed_va <= r[0] < feed_end]
    callsites = [address for address, kind in callers if kind == 'call']
    return_offsets = sorted(address + 5 for address in callsites)

    # ret instructions and panic calls inside feed.
    rets = [address for address, text in feed if text == 'ret']
    panics = [(address, text) for address, text in feed if 'panic' in text and text.startswith('call')]
    inside_drain = all(drain_va <= address < drain_end for address in callsites)

    # Prelude proof at each call site: rdi=sink(r14), rsi=grid(rbx), rdx=r12/r13=drain_rsp+0x18, rcx=length.
    index = {address: i for i, (address, _t) in enumerate(drain)}
    prelude = []
    rsp_writers = [(a, t) for a, t in drain if re.search(r'%rsp$', dest_register(t)) and not t.startswith(('push', 'pop'))]
    for site in CALL_SITES:
        i = index[site]
        window = drain[max(0, i - 40):i]
        moves = [t for _a, t in drain[i - 3:i]]
        buffer_register = {'mov    %r13,%rdx': '%r13', 'mov    %r12,%rdx': '%r12'}.get(moves[2])
        define = next((t for _a, t in reversed(window) if dest_register(t) == buffer_register), None)
        cmp_index = max((k for k, (_a, t) in enumerate(drain[:i]) if t.startswith('cmp    $0x200,%rcx')), default=None)
        test_index = max((k for k, (_a, t) in enumerate(drain[:i]) if t.startswith('test   %rcx,%rcx')), default=None)
        clobber = [t for _a, t in drain[test_index + 1:i] if dest_register(t) in RCX] if test_index is not None else None
        prelude.append({
            'call_address': site, 'return_offset': site + 5,
            'rdi_from_r14': moves[0] == 'mov    %r14,%rdi', 'rsi_from_rbx': moves[1] == 'mov    %rbx,%rsi',
            'rdx_move': moves[2], 'buffer_definition': define,
            'rdx_equals_drain_rsp_plus_0x18': bool(define and define.startswith('lea    0x18(%rsp),')),
            'length_bound_checked_before_call': cmp_index is not None and test_index is not None,
            'rcx_not_modified_between_zero_test_and_call': clobber == [],
            'compare_instruction': drain[cmp_index][1] if cmp_index is not None else None,
            'branch_after_compare': drain[cmp_index + 1][1] if cmp_index is not None else None,
            'branch_after_test': drain[test_index + 1][1] if test_index is not None else None})
    prologue_pushes = [t for _a, t in drain[:6]]
    frame = next(t for _a, t in drain if t.startswith('sub    $') and t.endswith(',%rsp'))
    stack_adjust = [(a, t) for a, t in rsp_writers]

    # Stream order proof: stage-1 endpoint is host+0xc8, stage-2 endpoint is host+0xe8 after d1.
    endpoint_reads = [(a, t) for a, t in drain if t.startswith('call') and '26e60' in t]
    stage = {'read_calls': [a for a, _t in endpoint_reads],
             'first_endpoint_lea': next(t for a, t in drain if a == 0x9d),
             'second_endpoint_adjust': next(t for a, t in drain if a == 0xd1),
             'second_first_read_uses_adjusted_r15': next(t for a, t in drain if a == 0xe2),
             'loop_endpoint_lea': next(t for a, t in drain if a == 0x165),
             'loop_counters': [t for a, t in drain if a in (0x14b, 0x1e5)]}

    first48 = data[va_to_offset(loads, 0):va_to_offset(loads, 0) + 48]
    cell_data = cellbox.read_bytes()
    _et, cell_loads, _bn = parse_elf(cell_data)
    cell48 = cell_data[va_to_offset(cell_loads, 0):va_to_offset(cell_loads, 0) + 48]
    reloc_offsets = sorted(r[0] for r in rela)

    def window(va, length, label):
        offset = va_to_offset(loads, va)
        clash = [r for r in reloc_offsets if va <= r < va + length]
        return {'label': label, 'va': va, 'length': length, 'hex': data[offset:offset + length].hex(),
                'relocation_free': not clash}
    windows = [window(0, 48, 'entry'), window(feed_va, 64, 'feed-prologue'),
               window(rets[0] - 0x11, 0x12, 'feed-ret-epilogue')]
    windows += [window(site + 5 - 14, 14, f'call-site-{site:#x}') for site in CALL_SITES]
    text_load = loads[0]
    return {
        'scope': 'static read-only derivation from the immutable candidate Terminal ELF; no execution',
        'terminal': {'path': str(terminal), 'sha256': terminal_hash, 'size': terminal.stat().st_size, 'etype': etype,
                     'loads': loads, 'text_load_flags': text_load['flags'], 'text_va_zero_is_slot_base': text_load['va'] == 0},
        'cellbox_sha256': cellbox_hash,
        'terminal_first48_hex': first48.hex(), 'cellbox_first48_hex': cell48.hex(),
        'entry_fingerprints_differ': first48 != cell48,
        'feed': {'symbol': FEED_SYMBOL, 'va': feed_va, 'size_from_symbol': feed_size, 'end_va': feed_end,
                 'kind': feed_kind, 'first_instruction': feed[0][1], 'first_instruction_bytes_hex': windows[1]['hex'][:2],
                 'ret_addresses': rets, 'panic_calls': panics,
                 'register_prologue': [t for _a, t in feed[:12]]},
        'drain': {'symbol': DRAIN_SYMBOL, 'va': drain_va, 'end_va': drain_end, 'frame_adjust': frame,
                  'prologue': prologue_pushes, 'rsp_writes': stack_adjust},
        'callers': {'all_direct_references': [{'address': a, 'kind': k} for a, k in callers],
                    'call_count': len(callsites), 'all_inside_drain': inside_drain,
                    'return_offsets': return_offsets, 'pointer_or_lea_references': pointer_refs,
                    'relocation_references': reloc_hits, 'expected_call_sites': list(CALL_SITES)},
        'call_site_prelude': prelude,
        'entry_invariants': {'rdx_equals_rsp_plus': 0x20, 'rcx_min': 1, 'rcx_max': 0x200,
                             'derivation': 'drain_rsp+0x18 buffer; call pushes 8 bytes => RDX==RSP_entry+0x20'},
        'stream_mapping': {'return_offset_to_stream_stage': {hex(k): v for k, v in RET_SITES.items()},
                           'stage1': 'host+0xc8 endpoint read, program-order first = source drain_stdout (output ring)',
                           'stage2': 'host+0xe8 endpoint read, only after stage 1 loop ends = drain_stderr (diagnostic ring)',
                           'method': 'program-order proof from asm plus session.rs for stderr in [false,true]; field names are NOT read from a struct layout',
                           'asm_evidence': stage},
        'windows': windows,
        'window_relocation_free': all(w['relocation_free'] for w in windows),
        'relocation_count': len(rela),
        'receipts': receipts,
    }


def main():
    result = derive()
    out = HERE / 'static-identity.json'
    out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'sha256': result['terminal']['sha256'], 'callers': result['callers']['call_count'],
                      'return_offsets': [hex(x) for x in result['callers']['return_offsets']],
                      'rets': result['feed']['ret_addresses']}))


if __name__ == '__main__':
    main()
