# Cellos Scripting Guide

Cellos currently ships one active native scripting runtime: **Lua 5.4**.
Older roadmap text records a MicroPython experiment, but MicroPython is not a
current Cargo workspace member and should not be documented as a supported
runtime.

---

## Lua 5.4

### Starting the REPL

At the shell prompt:
```
Cellos> lua
Lua 5.4 on Cellos  (Ctrl+D to exit)
> 
```

Use `Ctrl+D` on an empty line to exit.  `Ctrl+C` cancels the current input line.
Arrow-up/down navigates command history (session-local; persistence added in Phase 17a).

```
Cellos> lua /scripts/hello.lua
Cellos> lua -e 'print(vnet.resolve("gateway"))'
```

Lua runs the supplied VFS script or inline chunk; without arguments it starts
the REPL.

### Built-in Libraries

Lua 5.4 ships `string`, `table`, `math`, `coroutine`, and `package` with
VFS-backed `io.open`. The sandbox removes `io.popen`, `os.execute`,
`package.loadlib`, and `debug`.

```lua
-- String operations
local s = "Hello, Cellos!"
print(s:upper())          -- HELLO, Cellos!
print(#s)                 -- 12

-- Math
print(math.sqrt(144))     -- 12.0
print(math.floor(3.7))    -- 3

-- Table
local t = {1, 2, 3, "four"}
for i, v in ipairs(t) do
    print(i, v)
end
```

### VFS I/O

The `io.open` wrapper uses the current `vfs` IPC bindings. Read and buffered
write/append modes are available in the Lua runtime.

```lua
local f = io.open("/readme.txt", "r")
if f then
    local content = io.read(f, 4096)
    io.close(f)
    print(content)
else
    print("file not found")
end
```

Writes are committed through `vfs.write` or `vfs.append` when the handle closes.

### Network IPC

`vnet` delegates to `/bin/net`; Lua has no direct network device authority.
`vnet.connect(ip, port)` returns a TCP socket ID or `nil, error`.
`vnet.send(id, bytes)` returns the number of bytes accepted (possibly partial);
`vnet.recv(id[, max_bytes])` returns binary-safe bytes or `nil` after bounded
polling; close with `vnet.close(id)`. For UDP, use `vnet.udp_socket()`,
`vnet.udp_bind(id, port)`, `vnet.udp_send(id, ip, port, bytes)` and
`vnet.udp_recv(id[, max_bytes])` (returns source IP, source port, bytes).
`vnet.resolve(host)` returns a dotted IPv4 address through the net service.
Ports must be 1–65535, including the UDP bind port. The receive limit defaults
to 512 bytes; a UDP datagram sent from Lua is at most 512 bytes. Pending sends
and receives poll for up to 3 seconds (500 attempts if the clock is unavailable);
errors and timeouts return `nil` or zero as appropriate.

For an isolated RV64 end-to-end check, build the Lua cell with a compatible
RISC-V C/newlib toolchain, then run
`python3 scripts/qemu-lua-vnet.py --lua /path/to/release/lua`.
The runner signs a private copy and boots a private copy of `disk_v3.img`;
it checks DNS and binary-safe TCP/UDP against local host sockets.

### os.execute

`os.execute` is disabled (`nil`) by the Lua sandbox, together with `io.popen`
and the `debug` library. Native scripts cannot launch arbitrary shell commands.

### Multi-line Input

The Lua REPL handles incomplete chunks automatically — just keep typing:

```
> function greet(name)
>>   print("Hello, " .. name .. "!")
>> end
> greet("Cellos")
Hello, Cellos!
```

### Example Scripts

#### Fibonacci
```lua
local function fib(n)
    if n <= 1 then return n end
    return fib(n-1) + fib(n-2)
end
for i = 0, 10 do
    io.write(fib(i) .. " ")
end
print()
-- 0 1 1 2 3 5 8 13 21 34 55
```

#### Read and parse a config file
```lua
local f = io.open("/etc/hostname", "r")
if f then
    local name = io.read(f, 256)
    io.close(f)
    print("Hostname:", name)
end
```

---

## Python / MicroPython Status

Native MicroPython is historical only. For Python workloads, use the Tier 3
Linux VM path rather than assuming `/bin/python` exists in the native Cellos
image.

---

## Examples Directory

Place `.lua` scripts under `/scripts/` on the disk image.
Use `scripts/format-disk.ps1` to bake them in.

```
disk.img
└── scripts/
    ├── hello.lua
    └── fib.lua
```

---

## Adding New Lua C Bindings

1. Declare the Rust `extern "C" fn Cellos_xxx(L: *mut LuaState) -> c_int` in
   `cells/runtimes/lua/src/bindings_io.rs`.
2. Register it in `cells/runtimes/lua/glue/lua_Cellos_glue.c` via `lua_register(L, "xxx", Cellos_xxx)`.
3. Call any VFS/IPC operations using `ostd::syscall::sys_*` helpers.
4. Add the binding to the table in this document.

---

## Known Limitations (v0.2.1-dev)

| Feature | Status |
|---------|--------|
| **Lua** `io.open` read | ✅ Works (VFS IPC) |
| **Lua** `io.open` write | ✅ Works via vfs.write() IPC |
| **Lua** `os.execute` | Disabled by the runtime sandbox |
| **Python/MicroPython native runtime** | Historical only; use Tier 3 Linux VM for Python |
| **Lua** arg passing to scripts | ✅ Works (spawn_args early-read pattern) |
| **Lua** history persistence | 🚧 Phase 17a (VFS write) |
| **Lua** `require` / `package.path` | Stub (no VFS directory scan yet) |

---

## Shell Built-ins

| Built-in | Usage | Added |
|---|---|---|
| echo | Print text, supports $VAR expansion | Phase A |
| cat | Read file (via VFS) | Phase A |
| ls | List directory (via VFS) | Phase A |
| sleep N | Pause N seconds | Phase K |
| source / . | Execute script from VFS | Phase J |
| break / continue | Loop control | Phase R |
| exit N | Exit with code | Phase S |
| unset VAR | Remove variable | Phase S |
| test / [ | Condition testing (-f, -z, -n, =, !=) | Phase U |
| read | Read user input (Phase X-2) | Phase X-2 |
| wget URL path | Download URL body to VFS | Phase U |
| httpd port path | HTTP/1.0 file server | Phase M |

## Shell Advanced Features

| Feature | Usage | Added |
|---|---|---|
| `for var in list; do ... done` | Loop over items | Phase J |
| `while cond; do ... done` | While loop | Phase R |
| `if cond; then ... fi` | Conditional | Phase S |
| `case var in pattern) ... esac` | Case switch | Phase T |
| `name() { ... }` | Define shell function | Phase X-1 |
| `func $1 $2 $9` | Function arguments | Phase X-2 |
| `$(cmd)` | Command substitution | Phase X-3 |
| `\| grep` | Pipe commands | Phase E |
| `> file` | Redirect stdout to file | Phase E |
| `>> file` | Append stdout to file | Phase E |
| `& bg` | Background job | Phase F |
| `$VAR` | Variable expansion | Phase G |
| `$?` | Last exit code | Phase H |
| `||` and `&&` | Command chaining | Phase I |
