use crate::process::drivers::virtio_blk::BLOCK_DEVICE;

pub struct MiniFat32;

impl MiniFat32 {
    pub fn init() {
        log::info!("FAT: Initializing Mini Driver...");
        let mut guard = BLOCK_DEVICE.lock();
        
        if let Some(blk) = guard.as_mut() {
            let mut buf = [0u8; 512];
            // Read MBR/BS
            if blk.read_blocks(0, &mut buf).is_err() {
                log::error!("FAT: Failed to read Sector 0");
                return;
            }

            if buf[510] != 0x55 || buf[511] != 0xAA {
                log::error!("FAT: Invalid Signature");
                return;
            }

            let mut partition_lba = 0;
            // Check for FAT32 Signature at offset 82 ("FAT32   ") or FAT16 at 54 ("FAT16   ")
            let fat32_sig = &buf[82..87]; 
            let fat16_sig = &buf[54..59];
            
            let mut is_boot_sector = false;
            if fat32_sig == b"FAT32" || fat16_sig == b"FAT16" {
                is_boot_sector = true;
            }

            if !is_boot_sector {
                log::info!("FAT: Check MBR...");
                let p1 = &buf[0x1BE..0x1CE];
                let p_type = p1[4];
                let p_lba = u32::from_le_bytes([p1[8], p1[9], p1[10], p1[11]]);
                
                log::info!("Partition 1: Type=0x{:X}, LBA={}", p_type, p_lba);
                
                // 0x06=FAT16, 0x0B/0x0C=FAT32, 0x0E=FAT16 LBA
                if p_type == 0x06 || p_type == 0x0E || p_type == 0x0B || p_type == 0x0C {
                    partition_lba = p_lba;
                    if blk.read_blocks(partition_lba as usize, &mut buf).is_err() {
                        log::error!("FAT: Failed to read Partition BS");
                        return;
                    }
                } else {
                     log::error!("FAT: Partition 1 not FAT (Type {:X})", p_type);
                     return;
                }
            }

            // Parse BPB
            let sectors_per_cluster = buf[13] as u32;
            let reserved_sectors = u16::from_le_bytes([buf[14], buf[15]]) as u32;
            let num_fats = buf[16] as u32;
            let root_entries = u16::from_le_bytes([buf[17], buf[18]]); // 0 for FAT32
            
            let sectors_per_fat_16 = u16::from_le_bytes([buf[22], buf[23]]) as u32;
            let sectors_per_fat_32 = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
            
            // Heuristic for FAT32: RootEntries == 0
            let is_fat32 = (root_entries == 0);
            let sectors_per_fat = if is_fat32 { sectors_per_fat_32 } else { sectors_per_fat_16 };
            
            log::info!("FAT Type: {}, Sec/Clus={}, Res={}, RootEnt={}", 
                if is_fat32 { "FAT32" } else { "FAT16" }, sectors_per_cluster, reserved_sectors, root_entries);

            // Calculate Offsets
            let fat_start_lba = partition_lba + reserved_sectors;
            let root_dir_lba = fat_start_lba + (num_fats * sectors_per_fat);
            
            let mut data_start_lba = 0;
            let mut read_lba = 0;
            
            if is_fat32 {
                 let root_cluster = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]);
                 log::info!("FAT32 RootCluster: {}", root_cluster);
                 data_start_lba = root_dir_lba;
                 read_lba = data_start_lba + (root_cluster - 2) * sectors_per_cluster;
            } else {
                 let root_dir_sectors = (root_entries as u32 * 32 + 511) / 512;
                 data_start_lba = root_dir_lba + root_dir_sectors;
                 read_lba = root_dir_lba; // Root Dir IS at root_dir_lba
            }
            
            log::info!("Reading Root Dir at LBA {}", read_lba);
            
            let mut dir_buf = [0u8; 512];
            if blk.read_blocks(read_lba as usize, &mut dir_buf).is_ok() {
                // Parse Entries
                for i in 0..(512/32) {
                    let off = i * 32;
                    let entry = &dir_buf[off..off+32];
                    
                    if entry[0] == 0 { break; } 
                    if entry[0] == 0xE5 { continue; } 
                    
                    let attr = entry[11];
                    if attr == 0x0F { continue; } 
                    if (attr & 0x10) != 0 { 
                         // Check illegal chars in name
                         let name = core::str::from_utf8(&entry[0..11]).unwrap_or("DIR     ");
                         log::info!("DIR: {}", name);
                         continue;
                    }
                    
                    let name = &entry[0..8];
                    let ext = &entry[8..11];
                    let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);
                    let cluster_hi = u16::from_le_bytes([entry[20], entry[21]]);
                    let cluster_lo = u16::from_le_bytes([entry[26], entry[27]]);
                    let cluster = ((cluster_hi as u32) << 16) | (cluster_lo as u32);
                    
                    let name_str = core::str::from_utf8(name).unwrap_or("???");
                    let ext_str = core::str::from_utf8(ext).unwrap_or("???");
                    
                    log::info!("FILE: {}.{} (Size: {}, Cluster: {})", name_str.trim(), ext_str.trim(), size, cluster);
                    
                    if name.starts_with(b"HELLO") {
                         log::info!("Found HELLO.TXT! Reading...");
                         let file_lba = data_start_lba + (cluster - 2) * sectors_per_cluster;
                         let mut file_buf = [0u8; 512];
                         if blk.read_blocks(file_lba as usize, &mut file_buf).is_ok() {
                             let len = (size as usize).min(512);
                             let text = core::str::from_utf8(&file_buf[0..len]).unwrap_or("<binary>");
                             log::info!("CONTENT: '{}'", text);
                         }
                    }
                }
            } else {
                log::error!("FAT: Failed to read Root Dir");
            }

        } else {
             log::error!("FAT: Block Device not available.");
        }
    }
}
