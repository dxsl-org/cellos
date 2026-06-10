# VFS IPC API Reference

VFS Cell endpoint: fixed IPC port registered at boot by the kernel.  All messages use the
binary framing described below.  Responses are sent back to the caller's port.

---

## Message Format

```
byte[0]   = opcode  (u8)
byte[1]   = path_len (u8, 0–253)
byte[2..] = path    (UTF-8, not NUL-terminated, max 253 bytes)
```

Total request buffer: 512 bytes maximum.

---

## Opcodes

### OP_GET_FILE (0x01)

Return a pointer/length pair for a read-only file already mapped in VFS memory.
Intended for the kernel early-loader; prefer `OP_OPEN`/`OP_READ`/`OP_CLOSE` for
user-space callers.

**Request:** `[0x01, path_len, path…]`

**Response:** 16 bytes
```
bytes[0..8]  = data_ptr (u64 LE) — virtual address of file data
bytes[8..16] = data_len (u64 LE) — length in bytes
```
All-zero response means the file was not found or `path` is a directory.

---

### OP_LIST_DIR (0x02)

List directory entries as newline-separated names.

**Request:** `[0x02, path_len, path…]`

**Response:** up to 480 bytes
```
"child1\nchild2\nchild3\n"
```
Zero-length response means the path does not exist or is not a directory.

---

### OP_STAT (0x03)

Return metadata for a path.

**Request:** `[0x03, path_len, path…]`

**Response:** 16 bytes
```
bytes[0..8] = size   (u64 LE, 0 for directories)
bytes[8]    = is_dir (u8, 1 = directory, 0 = file)
bytes[9..16]= reserved (zeroed)
```

---

### OP_WRITE (0x04)

Write data to a file. Routes to RamFS (/tmp/*) or FAT16 (/data/*).

**Request:** `[0x04, path_len, content_len_lo, content_len_hi, path…, content…]`
- `content_len` is u16 little-endian (max 65535 bytes per write)
- Effective message cap: min(512, 4 + path_len + content_len) bytes

**Response:** 1 byte
```
0x00 = success
0x01 = error (parent not found, insufficient disk space, quota exceeded)
```

**Mount Points**:
- `/tmp/*` → RamFS (volatile, cleared on reboot)
- `/data/*` → FAT16 (persistent, survives reboot)

---

### OP_MKDIR (0x05)

Create an empty directory.  Parent directory must already exist.  Fails if the path
already exists (file or directory).

**Request:** `[0x05, path_len, path…]`

**Response:** 1 byte
```
0x00 = success
0x01 = error (parent not found, path exists, or parent is not a directory)
```

---

### OP_RMDIR (0x06)

Remove an empty directory.  Fails if the path does not exist, is a file, or the
directory is non-empty.

**Request:** `[0x06, path_len, path…]`

**Response:** 1 byte
```
0x00 = success
0x01 = error (not found, not a directory, or non-empty)
```

---

### OP_UNLINK (0x07)

Remove a regular file.  Fails if the path does not exist or is a directory (use
`OP_RMDIR` or `OP_RMDIR_RECURSIVE` for directories).

**Request:** `[0x07, path_len, path…]`

**Response:** 1 byte
```
0x00 = success
0x01 = error (not found or is a directory)
```

---

### OP_READ (0x08)

Read file bytes from a path. Limited to 480 bytes per request (fits in single IPC message).

**Request:** `[0x08, path_len, path…]`

**Response:** up to 480 bytes
```
Raw file bytes, truncated to 480.  Zero-length response means file not found or is a directory.
```

---

### OP_RMDIR_RECURSIVE (0x09)

Recursively delete a directory and all its contents. **Restricted to `/data/*` paths for safety.**

**Request:** `[0x09, path_len, path…]`

**Response:** 1 byte
```
0x00 = success
0x01 = error (not found, not a directory, path not under /data/, or I/O error)
```

---

### OP_APPEND (0x0A)

Seek to end of file and append data. Semantically equivalent to OP_WRITE but ensures append semantics.

**Request:** `[0x0A, path_len, content_len_lo, content_len_hi, path…, content…]`
- Same format as OP_WRITE
- Always seeks to end before writing

**Response:** 1 byte
```
0x00 = success
0x01 = error (parent not found, insufficient disk space)
```

---

## Mount Points

| Mount point | Backing  | Writable | Notes                                  |
|-------------|----------|----------|----------------------------------------|
| `/bin`      | RamFS    | No       | Embedded binaries (shell, lua, cat, etc.) |
| `/tmp`      | RamFS    | Yes      | Volatile scratch; cleared on reboot    |
| `/data`     | FAT16    | Yes      | Persistent storage on VirtIO disk (LBA 0–81919) |

---

## Quota Model

Each Cell has a default quota of **32 MiB** of total bytes written.  The VFS service
tracks `CellId → bytes_used` internally.  Write operations that would push usage over
the limit receive `OP_WRITE` error `0xff` with an eventual `ViError::PermissionDenied`
propagated through the OSTD wrapper.

---

## Error Semantics

| Response byte | Meaning           |
|---------------|-------------------|
| `0x00`        | Success           |
| `0x01`        | General error     |
| `0xff`        | Not supported yet |

For `OP_STAT` and `OP_GET_FILE`, a zero-filled response indicates "not found".

---

## Path Rules

- All paths must start with `/`.
- Maximum path length: 253 bytes (fits in one IPC message with 1-byte length field).
- No NUL bytes.
- Path traversal (`..`) is not resolved by the VFS service; callers must send canonical paths.
- Paths must not end with `/` except for the root `/`.

---

## OSTD Convenience API

Cell code should use `libs/ostd/src/fs.rs` rather than the raw IPC protocol:

```rust
// Open and read a file
let mut f = File::open("/etc/hostname")?;
let name = f.read_to_string()?;
f.close()?;

// Directory listing
for entry in read_dir("/bin")? {
    let name = core::str::from_utf8(&entry.name).unwrap_or("?");
    println!("{}", name);
}
```
