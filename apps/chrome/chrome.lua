-- /apps/chrome/chrome.lua
-- Dynamic hook script for Chrome Tier 3 App in CellOS

local chrome = {}

-- Pre-launch hook: executed before the MicroVM is spawned.
-- Allows inspecting/modifying the launch configuration and CLI arguments.
function chrome.pre_launch(config, args)
    local target_url = (args and args[1]) or "https://google.com"
    print("[chrome.lua] Initializing Chrome Tier 3 session")
    print("[chrome.lua] Target URL: " .. target_url)

    -- Append target URL to entrypoint
    config.entrypoint = config.entrypoint .. " " .. target_url
    return true
end

-- Post-exit hook: executed after the MicroVM terminates.
-- Can be used for cleanup, synchronizing cookies or bookmarks to CellOS VFS.
function chrome.post_exit(exit_code)
    print("[chrome.lua] Chrome session terminated with exit code: " .. tostring(exit_code))
end

return chrome
