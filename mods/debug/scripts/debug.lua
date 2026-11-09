-- Debug probe. Writes under <user dir>/debug/:
--   NNN-<tag>.png        screenshot of the window (needs the window visible and active)
--   NNN-<tag>-soft.png   the same view drawn in software (always works)
--   NNN-<tag>.txt        world-state dump: player, inventory, target, surface and height maps
--   events.log           every engine hook with the time and player cell
--
-- Keys: F9 capture now. F10 toggle automatic capture. F11 dump only.

local AUTO_INTERVAL = 5          -- seconds between automatic captures when on
local AUTO_LIMIT = 40            -- automatic captures per session, then it stops itself

local auto = false
local since = 0
local shots = 0
local auto_shots = 0
local elapsed = 0

local function event(line)
    local x, y, z = player_pos()
    append_file("events.log", string.format("t=%.1fs cell %d,%d,%d  %s",
        elapsed, math.floor(x), math.floor(y), math.floor(z), line))
end

local function capture(tag, with_images)
    shots = shots + 1
    local name = string.format("%03d-%s", shots, tag)
    dump_state(name)
    if with_images then
        screenshot(name)
        snapshot(name .. "-soft")
    end
    event("capture " .. name)
    show_text("debug: saved " .. name, 1.5)
end

function on_script_loaded()
    append_file("events.log", "")
    event("session start")
    log("debug probe: F9 capture, F10 automatic capture, F11 dump only; files under debug/")
end

function on_tick(dt)
    elapsed = elapsed + dt
    if key_just_pressed("F9") then
        capture("f9", true)
    end
    if key_just_pressed("F11") then
        capture("f11", false)
    end
    if key_just_pressed("F10") then
        auto = not auto
        since = 0
        show_text("debug: automatic capture " .. (auto and "on" or "off"), 2)
        event("automatic capture " .. tostring(auto))
    end
    if auto then
        since = since + dt
        if since >= AUTO_INTERVAL then
            since = 0
            auto_shots = auto_shots + 1
            capture("auto", true)
            if auto_shots >= AUTO_LIMIT then
                auto = false
                event("automatic capture stopped after " .. AUTO_LIMIT)
            end
        end
    end
end

function on_block_broken(x, y, z, block)
    event(string.format("broken %s at %d,%d,%d", block, x, y, z))
end

function on_block_placed(x, y, z, block)
    event(string.format("placed %s at %d,%d,%d", block, x, y, z))
end
