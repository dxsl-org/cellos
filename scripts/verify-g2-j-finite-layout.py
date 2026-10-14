#!/usr/bin/env python3
"""Source geometry and bounded proposal model; never runtime/durability approval."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ['libs/cellos-fs/src/format.rs', 'libs/cellos-fs/src/engine.rs',
           'libs/cellos-fs/src/allocator.rs', 'libs/cellos-fs/src/inode.rs',
           'libs/cellos-fs/src/disk.rs']


def certificate():
    text = (ROOT / SOURCES[0]).read_text()
    def number(name):
        match = re.search(r'pub const ' + name + r':\s*\w+\s*=\s*(\d+);', text)
        if not match:
            raise ValueError('unsupported source expression: ' + name)
        return int(match.group(1))
    block, inode, inline, name = [number(k) for k in
                                  ['BLOCK_SIZE', 'INODE_SIZE', 'INODE_PAYLOAD_SIZE', 'MAX_NAME_LEN']]
    slots = re.search(r'pub inode_blocks: \[u64; (\d+)\]', text)
    entry = re.search(r'pub const SIZE: usize = 8 \+ 1 \+ 1 \+ MAX_NAME_LEN;', text[text.index('impl DirEntryRecord'):])
    if not slots or not entry:
        raise ValueError('unsupported inode/directory source geometry')
    entry_size = 8 + 1 + 1 + name
    per_table = block // inode
    capacity = int(slots.group(1)) * per_table
    # root + cellos + journal + tx + cp + four tx slots + two cp slots
    directories, files = 11, 12
    total = directories + files
    tables = (total + per_table - 1) // per_table
    payload = 112 + 32 + 4 + 256 + 32
    commit = 112 + 40 + 32
    # Four payloads and two checkpoints cross inline428; commits/switches remain inline.
    data = 6 * ((payload + block - 1) // block) if payload > inline else 0
    volume_blocks = (64 * 1024 * 1024) // block
    bitmap_blocks = (volume_blocks + 32767) // 32768
    bitset_bytes = (volume_blocks + 7) // 8
    paths = ['/srv/cellos/journal/' + kind + '/' + str(i) + '/' + record
             for kind, count, records in [('tx', 4, ['payload', 'commit']),
                                          ('cp', 2, ['checkpoint', 'switch'])]
             for i in range(count) for record in records]
    assert all(len(component.encode()) <= name for path in paths for component in path.split('/') if component)
    assert max(4, 2) <= inline // entry_size
    assert total <= capacity
    # Each graph <= tables+data. Both old roots and complete candidate may be disjoint.
    candidate_peak = 3 * (tables + data) + 2 + bitmap_blocks
    geometry = {'block_bytes': block, 'inode_bytes': inode, 'inline_bytes': inline,
                'directory_entry_bytes': entry_size, 'directory_entries': inline // entry_size,
                'inode_capacity': capacity, 'session_directories_including_volume_root_and_parents': directories,
                'session_record_files': files, 'session_created_inodes': total,
                'inode_table_blocks_per_root': tables, 'external_record_data_blocks_per_root': data,
                'max_payload_bytes': payload, 'commit_bytes': commit,
                'selected_volume_blocks': volume_blocks, 'bitmap_blocks': bitmap_blocks,
                'three_graph_peak_blocks_including_reserved': candidate_peak,
                'four_bitsets_storage_bytes': 4 * bitset_bytes,
                'single_block_buffer_bytes': block,
                'record_buffer_bytes': payload,
                'remaining_inode_numbers_after_fresh_session': capacity - total}
    def validate(roots, allocated, candidate, reclaimed, unknown, charge, occupancy, bootstrap=False,
                 prior_reservations=frozenset(), allocation_events=None):
        protected = set().union(*roots)
        if len(roots) != (1 if bootstrap else 2):
            raise ValueError('missing recoverable root')
        if not (protected | candidate) <= allocated or candidate & protected or reclaimed & protected:
            raise ValueError('unreserved successor, reachable block reuse or early reclamation')
        if candidate & prior_reservations:
            raise ValueError('candidate overlaps outstanding reservation')
        if allocation_events is not None and (len(allocation_events) != len(set(allocation_events))
                                              or set(allocation_events) != candidate):
            raise ValueError('duplicate or inconsistent candidate assignment')
        if occupancy > 4 or not unknown <= charge:
            raise ValueError('capacity overflow or uncharged Unknown')
    old, active, fresh = {10, 11}, {11, 12}, {13, 14}
    allocated = old | active | fresh
    validate([old, active], allocated, fresh, set(), {'T'}, {'T'}, 4)
    validate([active], allocated, fresh, set(), {'T'}, {'T'}, 4, bootstrap=True)
    controls = []
    cases = [('overlap reuse', [old, active], allocated, {10}, set(), {'T'}, {'T'}, 4),
             ('early reclamation', [old, active], allocated, fresh, {12}, {'T'}, {'T'}, 4),
             ('missing old root', [active], allocated, fresh, set(), {'T'}, {'T'}, 4),
             ('capacity overflow', [old, active], allocated, fresh, set(), {'T'}, {'T'}, 5),
             ('uncharged Unknown', [old, active], allocated, fresh, set(), {'T'}, set(), 4),
             ('unreserved successor', [old, active], old | active, fresh, set(), {'T'}, {'T'}, 4)]
    for label, *args in cases:
        try:
            validate(*args)
        except ValueError as error:
            controls.append({'mutation': label, 'detected': True, 'reason': str(error)})
        else:
            raise AssertionError('undetected control: ' + label)
    for label, reservations, events in [
        ('outstanding reservation alias', {13}, [13, 14]),
        ('duplicate candidate assignment', set(), [13, 13, 14])]:
        try:
            validate([old, active], allocated, fresh, set(), {'T'}, {'T'}, 4,
                     prior_reservations=reservations, allocation_events=events)
        except ValueError as error:
            controls.append({'mutation': label, 'detected': True, 'reason': str(error)})
        else:
            raise AssertionError('undetected control: ' + label)
    def publication(before, successor, cut, after, reclaimed, resume):
        published = cut in ('FS2_reply_lost_valid', 'FS2_success')
        expected = [before[-1], successor] if published else ([before[-1]] if cut == 'superblock_torn' else before)
        if after != expected or (resume and cut != 'FS2_success'):
            raise ValueError('invalid publication transition or premature resume')
        if not successor <= allocated:
            raise ValueError('unreserved successor')
        protected = set().union(*after)
        if reclaimed & protected or (cut != 'FS2_success' and reclaimed):
            raise ValueError('premature reclamation')
    cuts = []
    for bootstrap in (True, False):
        before = [active] if bootstrap else [old, active]
        for cut in ['data_partial', 'bitmap_partial', 'FS1_failed', 'superblock_torn',
                    'FS2_reply_lost_absent', 'FS2_reply_lost_valid', 'FS2_success']:
            published = cut in ('FS2_reply_lost_valid', 'FS2_success')
            after = [active, fresh] if published else ([active] if cut == 'superblock_torn' else before)
            union = set().union(*after)
            reclaimed = set().union(*before) - union if cut == 'FS2_success' else set()
            resume = cut == 'FS2_success'
            publication(before, fresh, cut, after, reclaimed, resume)
            cuts.append({'bootstrap': bootstrap, 'cut': cut, 'protected_blocks': sorted(union),
                         'reclaimable_now': sorted(reclaimed), 'mutation_resume': resume,
                         'transition_validated': True})
    for label, cut, after, reclaimed, resume in [
        ('failed FS1 promotes successor', 'FS1_failed', [active, fresh], set(), False),
        ('ambiguous reply resumes mutation', 'FS2_reply_lost_valid', [active, fresh], set(), True),
        ('ambiguous reply reclaims old blocks', 'FS2_reply_lost_valid', [active, fresh], {10}, False)]:
        try:
            publication([old, active], fresh, cut, after, reclaimed, resume)
        except ValueError as error:
            controls.append({'mutation': label, 'detected': True, 'reason': str(error)})
        else:
            raise AssertionError('undetected control: ' + label)
    return {'format': 'g2-j-finite-proposal-certificate-v2',
            'scope': 'source geometry and conditional set model; not implementation or admission',
            'source_pins': [{'path': p, 'sha256': hashlib.sha256((ROOT / p).read_bytes()).hexdigest()} for p in SOURCES],
            'geometry': geometry, 'publication_cuts': cuts, 'negative_controls': controls,
            'session_rule': 'tx4 cp2 used once; no inode/slot cycling; refuse before new dispatch',
            'unproven': ['compiled stack/heap layouts', 'actual CoW/fault transitions', 'charge transfer producer',
                         'global1024 heap/8192 history/2048 ranges/128 obligations fit',
                         'authenticated floor/device durability', 'production profile and CP1/CP2']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    report = certificate()
    with args.out.open('x') as output:
        json.dump(report, output, indent=2)
        output.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
