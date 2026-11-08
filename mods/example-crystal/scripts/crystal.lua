-- Example mod script. Each `on_*` function is a hook the engine calls; see mods/README.md.

local broken = 0

function on_script_loaded()
    log("crystal mod loaded")
end

-- Every fifth crystal ore mined yields a bonus shard, and the first one teaches the recipe.
function on_object_broken(x, y, object)
    if object ~= "crystal_ore" then return end
    broken = broken + 1
    if broken == 1 then
        log("You feel the crystal hum. Three shards and an ingot make a blade.")
    end
    if broken % 5 == 0 then
        give_item("crystal_shard", 1)
    end
end

-- Lamps placed at night glow a little gift.
function on_object_placed(x, y, object)
    if object == "crystal_lamp" and is_night() then
        give_item("crystal_shard", 1)
    end
end

function on_day_start(day)
    log("day " .. day .. " dawns over the crystal field")
end
