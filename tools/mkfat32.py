import struct
import os
import sys

def create_fat32_image(output_path, files):
    # Parameters
    sector_size = 512
    sector_count = 1048576 # 512MB to ensure FAT32 cluster count (> 65525)
    reserved_sectors = 32
    fats = 2
    root_cluster = 2
    
    # Read all input files
    file_entries = []
    total_data_size = 0
    for src, dst in files:
        with open(src, 'rb') as f:
            data = f.read()
            file_entries.append({
                'src': src,
                'dst': dst,
                'data': data,
                'size': len(data)
            })
    
    # Calculate Cluster Size
    cluster_size = 4096 # 8 sectors
    sectors_per_cluster = cluster_size // sector_size
    
    # Calculate required FAT size
    # Total sectors - Reserved - FATs = Data Sectors
    # But we don't know FAT size yet.
    # Approx: clusters = total / spc
    approx_clusters = sector_count // sectors_per_cluster
    # entries = clusters. size = entries * 4.
    fat_size_bytes = approx_clusters * 4
    fat_sectors = (fat_size_bytes + sector_size - 1) // sector_size
    # align to 32 sectors for safety? Not needed but good.
    fat_sectors = (fat_sectors + 31) // 32 * 32
    data_start_sector = reserved_sectors + (fats * fat_sectors)
    
    with open(output_path, 'wb+') as f:
        # 1. Fill with zeros
        f.seek((sector_count * sector_size) - 1)
        f.write(b'\x00')
        f.seek(0)
        
        # 2. Boot Sector ... (Standard)
        f.write(b'\xEB\x58\x90') # Jump
        f.write(b'MSWIN4.1') # OEM
        f.write(struct.pack('<H', sector_size))
        f.write(struct.pack('<B', sectors_per_cluster))
        f.write(struct.pack('<H', reserved_sectors))
        f.write(struct.pack('<B', fats))
        f.write(struct.pack('<H', 0))
        f.write(struct.pack('<H', 0))
        f.write(b'\xF8')
        f.write(struct.pack('<H', 0))
        f.write(struct.pack('<H', 32)) # SectorsPerTrack
        f.write(struct.pack('<H', 64)) # Heads
        f.write(struct.pack('<I', 0))
        f.write(struct.pack('<I', sector_count))
        
        # FAT32 Ext
        f.write(struct.pack('<I', fat_sectors))
        f.write(struct.pack('<H', 0))
        f.write(struct.pack('<H', 0))
        f.write(struct.pack('<I', root_cluster))
        f.write(struct.pack('<H', 1)) # FS Info
        f.write(struct.pack('<H', 6)) # Backup BS
        f.write(b'\x00' * 12)
        f.write(struct.pack('<B', 0x80))
        f.write(b'\x00')
        f.write(b'\x29')
        f.write(struct.pack('<I', 0x12345678))
        f.write(b'NO NAME    ')
        f.write(b'FAT32   ')
        
        f.seek(510)
        f.write(b'\x55\xAA')
        
        # 3. FS Info
        f.seek(sector_size * 1)
        f.write(b'RRaA')
        f.seek(sector_size * 1 + 484)
        f.write(b'rrAa')
        f.write(struct.pack('<I', 0xFFFFFFFF))
        f.write(struct.pack('<I', 0xFFFFFFFF))
        f.write(b'\x00' * 12) # Reserved2
        f.write(b'\x00\x00\x55\xAA')
        
        # Read Boot Sector (Sector 0) and replicate to Backup Boot Sector (Sector 6)
        f.seek(0)
        boot_sector = f.read(512)
        f.seek(6 * sector_size)
        f.write(boot_sector)
        
        # 4. FAT Tables & Data Placement
        fat_data = bytearray(fat_sectors * sector_size)
        
        # Init 0, 1, 2
        struct.pack_into('<I', fat_data, 0, 0x0FFFFFF8)
        struct.pack_into('<I', fat_data, 4, 0x0FFFFFFF)
        struct.pack_into('<I', fat_data, 8, 0x0FFFFFFF) # Root Cluster 2 is EOC (Dir is in Cluster 2 only?)
        
        # Assign clusters to files
        current_data_cluster = 3 # Start allocating from 3
        
        # Root Dir is fixed at Cluster 2. We assume it fits in 1 cluster for simplicity (4KB = 128 entries).
        # We need to write Root Dir entries later.
        
        clusters_map = {} # dst -> start_cluster
        
        for entry in file_entries:
            size = entry['size']
            needed = (size + cluster_size - 1) // cluster_size
            start_cluster = current_data_cluster
            clusters_map[entry['dst']] = start_cluster
            
            for i in range(needed):
                next_c = current_data_cluster + 1
                if i == needed - 1:
                    next_c = 0x0FFFFFFF # EOC
                struct.pack_into('<I', fat_data, current_data_cluster * 4, next_c)
                current_data_cluster += 1
                
        # Write FATs
        f.seek(reserved_sectors * sector_size)
        f.write(fat_data)
        f.seek((reserved_sectors + fat_sectors) * sector_size)
        f.write(fat_data)

        # 5. Root Directory (Cluster 2)
        cluster2_offset = data_start_sector * sector_size
        f.seek(cluster2_offset)
        
        for entry in file_entries:
            name = entry['dst'].upper()
            if '.' in name:
                base, ext = name.split('.')
            else:
                base, ext = name, ""
            base = base[:8].ljust(8)
            ext = ext[:3].ljust(3)
            
            f.write(base.encode('ascii'))
            f.write(ext.encode('ascii'))
            f.write(b'\x20')
            f.write(b'\x00')
            f.write(b'\x00')
            f.write(struct.pack('<H', 0))
            f.write(struct.pack('<H', 0))
            f.write(struct.pack('<H', 0))
            f.write(struct.pack('<H', 0)) # High Cluster 0
            f.write(struct.pack('<H', 0))
            f.write(struct.pack('<H', 0))
            
            start = clusters_map[entry['dst']]
            f.write(struct.pack('<H', start)) # Low Cluster
            f.write(struct.pack('<I', entry['size']))

        # 6. Write File Data
        for entry in file_entries:
            start = clusters_map[entry['dst']]
            offset = data_start_sector * sector_size + (start - 2) * cluster_size
            f.seek(offset)
            f.write(entry['data'])

    print(f"Created FAT32 image at {output_path} with {len(files)} files.")

if __name__ == "__main__":
    if len(sys.argv) < 2: # Need at least output
        print("Usage: mkfat32.py <output_img> [<src> <dst>]...")
        sys.exit(1)
        
    out = sys.argv[1]
    files = []
    args = sys.argv[2:]
    for i in range(0, len(args), 2):
        if i+1 < len(args):
            files.append((args[i], args[i+1]))
            
    create_fat32_image(out, files)
