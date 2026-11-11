-- A level pad around the spawn before the other scenarios run. The engine no longer steps
-- up single blocks by itself, so a walk test on wild terrain would stop at the first lip
-- for reasons that have nothing to do with what it checks. Scenarios that want the wild
-- world (streaming far away) teleport out of the pad themselves.

local SIZE = 6         -- half-width in cells: a 13 x 13 pad
local CLEAR = 8        -- cells of air kept above the floor

test("the spawn is flattened into a pad for the scenarios that follow", function()
    expect(wait_until(is_grounded, 300), "the player has landed")
    local px, py, pz = player_pos()
    local cx, cy, floor = math.floor(px), math.floor(py), math.floor(pz) - 1
    local placed, cleared = 0, 0
    for dx = -SIZE, SIZE do
        for dy = -SIZE, SIZE do
            local x, y = cx + dx, cy + dy
            -- Solid ground two deep, grass on top; air above.
            for z = floor - 2, floor - 1 do
                if get_block(x, y, z) ~= "stone" and set_block(x, y, z, "stone") then placed = placed + 1 end
            end
            if get_block(x, y, floor) ~= "grass" and set_block(x, y, floor, "grass") then placed = placed + 1 end
            for z = floor + 1, floor + CLEAR do
                if get_block(x, y, z) ~= "air" and set_block(x, y, z, "air") then cleared = cleared + 1 end
            end
        end
    end
    teleport(cx + 0.5, cy + 0.5, floor + 1)
    expect(wait_until(is_grounded, 120), "standing on the pad")
    local _, _, z = player_pos()
    expect(near(z, floor + 1, 0.01), string.format("at its level (%.2f)", z))
    log(string.format("arena: %d blocks placed, %d cleared around %d,%d at z %d", placed, cleared, cx, cy, floor))
end)
