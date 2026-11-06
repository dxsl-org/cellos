-- bytecode-refusal-test.lua: precompiled chunks must never reach the
-- interpreter (Lua has no bytecode verifier); source text must still load.
-- Run: lua /tmp/bytecode-refusal-test.lua
-- Written without double quotes, backslashes or trailing comments so a host
-- harness can also pass it as one `lua -e` line (comment lines dropped) when
-- the cell cannot read VFS files (a shell-launched child is VFS-sealed).
-- The script also writes /tmp/bytecode-top.lua when VFS allows it; then
-- `lua /tmp/bytecode-top.lua` must report a refused binary chunk.

print('=== bytecode-refusal-test on ViCell Lua 5.4 ===')

local passed, failed = 0, 0
local function check(name, ok, detail)
  if ok then
    passed = passed + 1
    print('  PASS: ' .. name)
  else
    failed = failed + 1
    print('  FAIL: ' .. name .. ' - ' .. tostring(detail))
  end
end

-- Lua 5.4 precompiled-chunk header. Mode 't' refuses on the first byte, so a
-- header is enough to prove the undumper is never reached; without the fix
-- it reports a bad binary format instead.
local BYTECODE = string.char(27, 76, 117, 97, 84, 0, 25, 147, 13, 10, 26, 10, 4, 8, 8) .. string.rep(string.char(0), 24)

local function refused(f, err)
  return f == nil and tostring(err):find('binary chunk', 1, true) ~= nil, err
end

check('load: default mode refuses bytecode', refused(load(BYTECODE)))
check('load: explicit b refused', refused(load(BYTECODE, '=x', 'b')))
check('load: explicit bt refused', refused(load(BYTECODE, '=x', 'bt')))
local fed = false
local function reader()
  if fed then return nil end
  fed = true
  return BYTECODE
end
check('load: reader function refuses bytecode', refused(load(reader)))
check('loadfile: explicit b refused', refused(loadfile('/tmp/json.lua', 'b')))

check('string.dump removed', string.dump == nil and ('').dump == nil)
check('debug library unreachable', debug == nil and package.loaded.debug == nil and not pcall(require, 'debug'))

-- The VFS require searcher reads through io.open -> vfs.read; serving the
-- module bytes from vfs.read keeps this independent of VFS reachability.
local real_read = vfs.read
local modules = { ['/tmp/bytecode_mod.lua'] = BYTECODE, ['/tmp/text_mod.lua'] = 'return { v = 42 }' }
vfs.read = function(path) return modules[path] or real_read(path) end
local ok, err = pcall(require, 'bytecode_mod')
check('require: bytecode module refused', not ok and tostring(err):find('binary chunk', 1, true) ~= nil, err)
ok, err = pcall(require, 'text_mod')
check('require: text module loads', ok and type(err) == 'table' and err.v == 42, err)
vfs.read = real_read

local f = load('return 1 + 1')
check('load: text chunk runs', f ~= nil and f() == 2)
f = load('return x', '=env', 't', { x = 7 })
check('load: text with env', f ~= nil and f() == 7)
f = load('return _ENV', '=noenv', 't', nil)
check('load: explicit nil env kept', f ~= nil and f() == nil)

vfs.write('/tmp/bytecode-top.lua', BYTECODE)

print(string.format('=== %d passed, %d failed ===', passed, failed))
print(failed == 0 and 'LUA_BYTECODE_REFUSAL_PASS' or 'LUA_BYTECODE_REFUSAL_FAIL')
