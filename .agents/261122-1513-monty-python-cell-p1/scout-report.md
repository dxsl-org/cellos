# Scout report: Monty P1

Recon 2026-11-22 at `943552a03d`.

| Topic | Fact | Source |
|---|---|---|
| PAL heap | Fixed `HEAP_SIZE = 4 MiB` static array (`HEAP_MEM`) with a first-fit free list behind `HEAP_LOCK` | `patches/rust-std-cellos.patch` (`pal/cellos/alloc.rs`) |
| Cell quota | `DEFAULT_QUOTA_BYTES = 16 MiB`, which is both the default and the ceiling; there is no per-path quota (the catalog has only `guest_memory`) | `kernel/src/memory/cell_quota.rs:34-46`; `kernel/src/loader/governed_spawn.rs:226`; `kernel/src/catalog.rs:39` |
| Large stack | `task::user_stack_pages_for(path, kernel_resolved, name)` matches only `/bin/monty-smoke` | `kernel/src/task.rs:711-725` |
| Monty memory hook | `max_memory` reads `monty_types::resource::LIVE_MEMORY`, which only a counting global allocator updates. Without one the limit is "silently not enforced" | `third_party/monty-src/crates/monty-types/src/resource.rs:31,386` |
| monty-alloc | A `GlobalAlloc` wrapper over `System`; a hard limit ends the process through `std::process` | `third_party/monty-src/crates/monty-alloc/src/lib.rs` |
| Monty OS calls | `Path.{exists,is_file,is_dir,is_symlink,read_text,read_bytes,stat,iterdir,scan,resolve,absolute,write_text,append_text,write_bytes,append_bytes,write_bytes_at,create_exclusive,mkdir,unlink,rmdir,rename}`, `open`, `os.getenv`, `os.environ`, `date.today`, `datetime.now`, `os.urandom`, `time.time`, `time.sleep`, `asyncio.sleep`, `import`, `import_stubs`, `typing.get_type_hints` | `monty-types/src/os.rs:54+` |
| Imports | `OsFunctionCall::Import` carries candidate paths; the host answers `ExtFunctionResult::ModuleFile` with the `.py` source | `crates/monty/README.md` |
| VFS client | `ostd::clients::vfs`: `read_file`, `read_file_bounded`, `read_chunk`, `write_file`, `stat`, `mkdir`, … | `libs/ostd/src/clients/vfs.rs:48-176` |
| REPL | `ostd::repl::Repl` (Read fd 0 + RecvTimeout); the Lua cell uses it | `libs/ostd/src/repl.rs:83`; `cells/runtimes/lua/src/main.rs:21-28` |
| Net split precedent | `/bin/lua` is `target()` (no network); `/bin/lua-net` is `net_client()`, the same signed binary, right chosen by the resolved path | `kernel/src/catalog.rs:544-562`; `docs/guides/tier1b-lua.md` |
| P0 witness | `cells/demos/monty-smoke` (entropy shim, case runner); `scripts/{build-x86_64-monty-smoke-ci.sh,x86/qemu-monty-smoke.sh}` | P0 plan phase 6 |
| Signed-ELF limit | The kernel copies the ELF minus its signature into its heap to verify it; 12.9 MB failed and 8.2 MB passed | `kernel/src/signing.rs:237-255` |
| PAL governance tool | `scripts/rust_std_promotion/regen_pal_inventory.py` (`--write`, then `--bind-records` after signing) | P0 phase 3 |
