-- /apps/python/python.lua
-- Dynamic hook script for Python Tier 3 App in CellOS

local py = {}

function py.pre_launch(config, args)
    print("[python.lua] Initializing CPython 3 MicroVM session")
    return true
end

function py.post_exit(exit_code)
    print("[python.lua] CPython session terminated with exit code: " .. tostring(exit_code))
end

return py
