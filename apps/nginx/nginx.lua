-- /apps/nginx/nginx.lua
-- Dynamic hook script for Nginx Tier 3 MicroVM in CellOS

local nginx = {}

function nginx.pre_launch(config, args)
    print("[nginx.lua] Initializing Nginx Tier 3 MicroVM service")
    return true
end

function nginx.post_exit(exit_code)
    print("[nginx.lua] Nginx terminated with exit code: " .. tostring(exit_code))
end

return nginx
