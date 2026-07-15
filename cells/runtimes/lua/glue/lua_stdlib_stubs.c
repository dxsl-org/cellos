/* Stubs for Lua stdlib openers whose C implementations cannot link in a
 * PIE cell.
 *
 * loslib.c pulls strftime/localtime from picolibc/newlib, whose
 * locale.o/timelocal.o carry absolute R_RISCV_64 relocations
 * (_C_time_locale) — rust-lld rejects them when linking the cell -pie.
 * liolib.c similarly drags in the FILE* stdio machinery.
 *
 * The ViCell Lua runtime never needs either: the sandbox prelude in
 * src/main.rs (inject_io_setup) rebuilds io.write / io.open on top of the
 * VFS bindings and explicitly nils io.popen / os.execute, and the runtime
 * exposes time via sys_get_time bindings instead of os.time. Registering
 * empty tables keeps linit.c's loadedlibs table intact.
 *
 * Mirrors cells/demos/tetris-lua/src/c/lua_game_stubs.c, which proved the
 * approach: with these two libs excluded, the remaining picolibc objects
 * link cleanly into a PIE cell.
 */

/* Forward-declare only what linit.c needs — avoids including lua.h here. */
typedef struct lua_State lua_State;

extern void lua_createtable(lua_State *L, int narr, int nrec);

int luaopen_io(lua_State *L) {
    lua_createtable(L, 0, 0); /* empty io — prelude installs VFS-backed io.* */
    return 1;
}

int luaopen_os(lua_State *L) {
    lua_createtable(L, 0, 0); /* empty os — os.* is sandboxed out */
    return 1;
}
