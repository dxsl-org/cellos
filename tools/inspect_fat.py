"""inspect_fat.py - inspect FAT16 image directory structure."""
import struct, os, sys

path = sys.argv[1] if len(sys.argv) > 1 else 'kernel/src/embedded-aarch64/kernel_fs.img'
print('Checking:', path)
sz = os.path.getsize(path)
print('Size:', sz, 'bytes (%d KB)' % (sz // 1024))

with open(path, 'rb') as f:
    data = f.read()

bs = data[:512]
print('Sig:', bs[510:512].hex(), '(should be 55aa)')
print('FS type:', bs[54:62])
bps = struct.unpack_from('<H', bs, 11)[0]
spc = bs[13]
res_s = struct.unpack_from('<H', bs, 14)[0]
fc = bs[16]
re = struct.unpack_from('<H', bs, 17)[0]
fat_sz16 = struct.unpack_from('<H', bs, 22)[0]
fat_sz32 = struct.unpack_from('<I', bs, 36)[0]
fat_sz = fat_sz16 if fat_sz16 else fat_sz32
print('BPS=%d SPC=%d RES=%d FAT_CNT=%d ROOT_ENT=%d FAT_SZ=%d' % (bps, spc, res_s, fc, re, fat_sz))

root_off = (res_s + fc * fat_sz) * bps
data_off = root_off + re * 32
print('Root dir at %d, Data area at %d' % (root_off, data_off))


def parse_dir(data, offset, label):
    print('  --- %s ---' % label)
    pending_lfn = []
    for i in range(512):
        e = data[offset + i*32: offset + (i+1)*32]
        if len(e) < 32 or e[0] == 0:
            break
        if e[0] == 0xE5:
            continue
        attr = e[11]
        if attr == 0x0F:
            # LFN entry - collect name fragments
            seq = e[0] & 0x3F
            n1 = e[1:11].decode('utf-16-le', errors='replace').rstrip('￿')
            n2 = e[14:26].decode('utf-16-le', errors='replace').rstrip('￿')
            n3 = e[28:32].decode('utf-16-le', errors='replace').rstrip('￿')
            frag = (n1 + n2 + n3).rstrip('\x00')
            print('    LFN[%d] %r' % (seq, frag))
            pending_lfn.append((seq, frag))
            continue
        name = e[:8].rstrip(b' ')
        ext = e[8:11].rstrip(b' ')
        size = struct.unpack_from('<I', e, 28)[0]
        clus = struct.unpack_from('<H', e, 26)[0]
        full = name + (b'.' + ext if ext else b'')
        # Reconstruct LFN if pending
        if pending_lfn:
            sorted_frags = sorted(pending_lfn, key=lambda x: x[0])
            lfn_full = ''.join(f for _, f in sorted_frags)
            print('    SFN %-12s  -> LFN %r  attr=%02x clus=%d sz=%d' % (
                full.decode(errors='replace'), lfn_full, attr, clus, size))
            pending_lfn = []
        else:
            print('    SFN %-20s attr=%02x clus=%d sz=%d' % (full.decode(errors='replace'), attr, clus, size))


parse_dir(data, root_off, 'root')

# Walk every subdirectory, not just /bin: images are packed from a list that can put entries
# anywhere (`mkfat32.py <src> /mnt/sd/ai-model.gguf`), and an inspector that silently ignores a
# directory makes a packed file look absent to every caller that greps this output.
def subdirectories(data, offset, entries):
    """Return (long name when present, short name, first cluster) for each subdirectory."""
    found = []
    pending = ''
    for i in range(entries):
        e = data[offset + i*32: offset + (i+1)*32]
        if e[0] == 0:
            break
        if e[0] == 0xE5:
            continue
        if e[11] == 0x0F:
            n1 = e[1:11].decode('utf-16-le', errors='replace').rstrip('￿')
            n2 = e[14:26].decode('utf-16-le', errors='replace').rstrip('￿')
            n3 = e[28:32].decode('utf-16-le', errors='replace').rstrip('￿')
            pending = (n1 + n2 + n3).rstrip('\x00') + pending
            continue
        name = e[:8].rstrip(b' ')
        long_name = pending
        pending = ''
        if (e[11] & 0x10) and name not in (b'.', b'..'):
            found.append((long_name, name, struct.unpack_from('<H', e, 26)[0]))
    return found

seen = set()
def label(long_name, short_name):
    return long_name if long_name else short_name.decode(errors='replace')

for long_name, short_name, clus in subdirectories(data, root_off, re):
    if clus < 2 or clus in seen:
        continue
    seen.add(clus)
    offset = data_off + (clus - 2) * spc * bps
    top = label(long_name, short_name)
    print('%s dir (SFN=%r) at cluster %d, offset %d' % (top, short_name.decode(errors='replace'), clus, offset))
    parse_dir(data, offset, '/' + top)
    for sub_long, sub_short, sub_clus in subdirectories(data, offset, re):
        if sub_clus < 2 or sub_clus in seen:
            continue
        seen.add(sub_clus)
        sub_off = data_off + (sub_clus - 2) * spc * bps
        sub = label(sub_long, sub_short)
        print('%s dir (SFN=%r) at cluster %d, offset %d' % (sub, sub_short.decode(errors='replace'), sub_clus, sub_off))
        parse_dir(data, sub_off, '/' + top + '/' + sub)
