-- Debug probe. Writes under <user dir>/debug/:
--   NNN-<tag>.png   screenshot of the window
--   NNN-<tag>.txt   world-state dump: clocks, player, inventory, creatures, chests, ASCII maps
--   events.log      every engine hook with day/time and player cell
--
-- Keys: F9 snapshot now. F10 toggle automatic snapshots (interval below). F11 dump only, no
-- screenshot. Automatic capture from launch, for runs nobody is watching: set the environment
-- variable FIELDSTONE_DEBUG_AUTO=<seconds>, or write the seconds into a file debug/auto next
-- to settings.ron (handy when the game is started with `open`, which passes no environment).
-- Screenshots need the window on screen and the app active; a game launched from a
-- background shell on macOS saves black images.

local AUTO_INTERVAL = 5          -- seconds between automatic snapshots when on
local AUTO_LIMIT = 40            -- automatic snapshots per session, then it stops itself

local auto = false
local since = 0
local shots = 0
local auto_shots = 0
local elapsed = 0

local function env_number(name)
    if os and os.getenv then
        return tonumber(os.getenv(name) or "")
    end
    return nil
end

-- Seconds from the debug/auto flag file, if the Lua io library is available and it exists.
local function auto_file()
    if not io or not io.open then
        return nil
    end
    local f = io.open("debug/auto", "r")
    if not f then
        return nil
    end
    local n = tonumber(f:read("*l") or "")
    f:close()
    return n
end

local function stamp()
    return string.format("day %d %.3f t=%.1fs cell %d,%d", 0, day_frac(), elapsed, player_x(), player_y())
end

local function event(line)
    append_file("events.log", stamp() .. "  " .. line)
end

local function snapshot(tag, with_image)
    shots = shots + 1
    local name = string.format("%03d-%s", shots, tag)
    dump_state(name)
    if with_image then
        screenshot(name)
    end
    event("snapshot " .. name)
    show_text("debug: saved " .. name, 1.5)
end

function on_script_loaded()
    local interval = env_number("FIELDSTONE_DEBUG_AUTO") or auto_file()
    if interval and interval > 0 then
        AUTO_INTERVAL = interval
        auto = true
    end
    append_file("events.log", "")
    event("session start, auto=" .. tostring(auto) .. " every " .. AUTO_INTERVAL .. "s")
    log("debug probe: F9 snapshot, F10 auto capture, F11 dump only; files under debug/")
end

function on_tick(dt)
    elapsed = elapsed + dt
    if key_just_pressed("F9") then
        snapshot("f9", true)
    end
    if key_just_pressed("F11") then
        snapshot("f11", false)
    end
    if key_just_pressed("F10") then
        auto = not auto
        since = 0
        show_text("debug: auto capture " .. (auto and "on" or "off"), 2)
        event("auto capture " .. tostring(auto))
    end
    if auto then
        since = since + dt
        if since >= AUTO_INTERVAL then
            since = 0
            auto_shots = auto_shots + 1
            snapshot("auto", true)
            if auto_shots >= AUTO_LIMIT then
                auto = false
                event("auto capture stopped after " .. AUTO_LIMIT .. " snapshots")
            end
        end
    end
end

function on_object_broken(x, y, object)
    event(string.format("broken %s at %d,%d", object, x, y))
end

function on_object_placed(x, y, object)
    event(string.format("placed %s at %d,%d", object, x, y))
end

function on_object_used(x, y, from, to)
    event(string.format("used %s -> %s at %d,%d", from, to, x, y))
end

function on_craft(recipe)
    event("crafted " .. recipe)
end

function on_death(who, x, y)
    event(string.format("died %s at %d,%d", who, x, y))
    snapshot("death", true)
end

function on_day_start(day)
    event("day " .. day .. " begins")
end
