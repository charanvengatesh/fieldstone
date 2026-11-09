-- Reacts to the engine's hooks. Each script has its own globals, so these names cannot
-- clash with another mod's.

local found = 0

function on_script_loaded()
    log("crystal mod loaded")
end

function on_block_broken(x, y, z, block)
    if block ~= "crystal_ore" then
        return
    end
    found = found + 1
    if found == 1 then
        show_text("Your first crystal. Place it anywhere: it is a block like any other.", 5)
    elseif found % 10 == 0 then
        give_item("crystal", 1)
        show_text(found .. " crystals mined. Have one more.", 3)
    end
end
