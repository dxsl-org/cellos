-- Sandbox prelude: runs once, before any user code, from inject_io_setup().
--
-- Invariant: only Lua *source text* ever reaches the compiler. Lua has no
-- bytecode verifier; a crafted precompiled chunk ("\27Lua...") yields
-- arbitrary memory read/write inside the interpreter. Every chunk entry point
-- reachable from a script is therefore pinned to mode 't' here, and the Rust
-- loaders pass 't' too (ffi::load_text).
--
-- The raw loaders survive only as upvalues of the wrappers below. That is
-- sound only while no script can reach the debug library (getupvalue would
-- hand them back), so `debug` is removed from both _G and package.loaded.

local raw_load = load
local raw_loadfile = loadfile
local BINARY_REFUSED = 'attempt to load a binary chunk (only text chunks are allowed)'

-- `load`/`loadfile` distinguish an absent env argument from an explicit nil
-- (an explicit nil env leaves the chunk without _ENV), so trailing arguments
-- are forwarded with their original count.
load = function(chunk, chunkname, mode, ...)
  if mode ~= nil and mode ~= 't' then return nil, BINARY_REFUSED end
  return raw_load(chunk, chunkname, 't', ...)
end

loadfile = function(filename, mode, ...)
  if mode ~= nil and mode ~= 't' then return nil, BINARY_REFUSED end
  return raw_loadfile(filename, 't', ...)
end

dofile = function(filename)
  local f, err = raw_loadfile(filename, 't')
  if not f then error(err, 2) end
  return f()
end

io.write = function(...)
  for _, v in ipairs({...}) do ViCell_io_write(tostring(v)) end
end
-- Dangerous stdlib removed to enforce the sandbox boundary:
--   io.popen        spawns a process (impossible in this cell, removed anyway)
--   os.execute      arbitrary commands
--   debug           inspects/mutates interpreter state, including upvalues
--   string.dump     produces bytecode; nothing here may consume it
--   package.loadlib loads native C libraries (no dlopen, removed anyway)
io.popen    = nil
os.execute  = nil
debug       = nil
string.dump = nil
if package then
  package.loaded.debug = nil
  package.loadlib = nil
  -- Override the file-system searcher to use VFS-backed io.open instead of
  -- C fopen (which calls ViSyscall::Open, not in this cell's allowlist).
  -- Scripts are installed to /tmp at startup via install_bundled_scripts().
  package.path = '/tmp/?.lua'
  package.searchers = {
    package.searchers[1],
    function(name)
      local path = '/tmp/' .. name:gsub('%.', '/') .. '.lua'
      local f = io.open(path, 'r')
      if not f then return 'module ' .. name .. ' not found at ' .. path end
      local src = f:read('*a')
      f:close()
      local chunk, err = raw_load(src, '@' .. path, 't')
      -- A searcher's (nil, msg) is silently dropped by require; raise like
      -- the stock Lua searcher so a refused or malformed module is visible.
      if not chunk then
        error('error loading module ' .. name .. ' from file ' .. path .. ':\n\t' .. err, 0)
      end
      return chunk, path
    end,
  }
end
io.open = function(path, mode)
  mode = mode or 'r'
  if mode == 'r' or mode == 'rb' then
    local d = vfs.read(path)
    if d == nil then return nil, 'cannot open: ' .. tostring(path) end
    local h = {_d = d, _p = 1}
    local NL = string.char(10)
    function h:read(f)
      f = f or '*l'
      if f == '*a' or f == '*all' then
        self._p = #self._d + 1
        return self._d
      end
      local s, e = string.find(self._d, NL, self._p, true)
      if s then
        local line = string.sub(self._d, self._p, s - 1)
        self._p = e + 1
        return line
      elseif self._p <= #self._d then
        local line = string.sub(self._d, self._p)
        self._p = #self._d + 1
        return line
      end
      return nil
    end
    function h:close() return true end
    return h
  elseif mode == 'w' or mode == 'wb' then
    local h = {_path = path, _buf = ''}
    function h:write(...)
      for _, v in ipairs({...}) do self._buf = self._buf .. tostring(v) end
      return self
    end
    function h:close() vfs.write(self._path, self._buf); return true end
    return h
  elseif mode == 'a' then
    local h = {_path = path, _buf = ''}
    function h:write(...)
      for _, v in ipairs({...}) do self._buf = self._buf .. tostring(v) end
      return self
    end
    function h:close() vfs.append(self._path, self._buf); return true end
    return h
  end
  return nil, 'unsupported mode: ' .. tostring(mode)
end
