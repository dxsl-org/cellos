
import sys
import struct

def check_elf(path):
    with open(path, 'rb') as f:
        data = f.read(64)
        
    # Check Magic
    if data[0:4] != b'\x7fELF':
        print("Not an ELF file")
        return

    # 64-bit check (EI_CLASS should be 2)
    if data[4] != 2:
        print("Not a 64-bit ELF")
        
    # Endian (EI_DATA should be 1 for Little)
    is_little = data[5] == 1
    endian = '<' if is_little else '>'
    
    # Parse Header (Elf64_Ehdr)
    # e_entry is at offset 24 (0x18), 8 bytes
    entry = struct.unpack(endian + 'Q', data[24:32])[0]
    
    print(f"ELF: {path}")
    print(f"Entry Point: 0x{entry:X}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python check_elf.py <path>")
        sys.exit(1)
    check_elf(sys.argv[1])
