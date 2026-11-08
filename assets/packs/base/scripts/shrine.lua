-- The base game's goal, written as a script so mods can replace or extend it.
-- Build a Sun Shrine, forge three Sun Cores at a furnace, and feed them to the shrine.

local remaining = { shrine_1 = 2, shrine_2 = 1 }

function on_object_placed(x, y, object)
    if object == "shrine_0" then
        show_text("The shrine waits for three Sun Cores.", 5)
    end
end

function on_object_used(x, y, from, to)
    if remaining[to] then
        show_text("The shrine drinks the light. " .. remaining[to] .. " to go.", 4)
    elseif to == "shrine_3" then
        win("Three suns burn in the shrine. Night will not come to this field again. Your base stands.")
    end
end

function on_day_start(day)
    if day == 3 then
        show_text("Day 3. Gold in the hills, a shrine in your plans.", 5)
    end
end
