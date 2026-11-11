-- Breaking and placing through the pointer, with the engine's own hooks as witnesses.

local broken, placed = nil, nil
function on_block_broken(x, y, z, block) broken = { x = x, y = y, z = z, block = block } end
function on_block_placed(x, y, z, block) placed = { x = x, y = y, z = z, block = block } end

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

-- A block near the player whose top face the pointer can reach, at least `min_r` cells
-- away (building right beside the player can put the block inside them, which the engine
-- refuses).
local function find_block(min_r)
    local px, py = player_pos()
    px, py = math.floor(px), math.floor(py)
    for r = min_r or 1, 4 do
        for dy = -r, r do
            for dx = -r, r do
                if math.max(math.abs(dx), math.abs(dy)) == r then
                    local x, y = px + dx, py + dy
                    local z = top_of(x, y)
                    if z then
                        aim_at(x + 0.3, y + 0.6, z + 1)
                        wait(1)
                        local t = target()
                        if t and t.x == x and t.y == y and t.z == z and t.nz == 1
                            and t.block ~= "water" and t.block ~= "bedrock" then
                            return x, y, z, t.block
                        end
                    end
                end
            end
        end
    end
    return nil
end

test("holding the pointer on a block breaks it and the drop lands in the bag", function()
    local x, y, z, block = find_block()
    expect(x ~= nil, "found a block to break")
    if not x then return end
    press_mouse("left")
    local gone = wait_until(function() return get_block(x, y, z) == "air" end, 400)
    release_mouse("left")
    clear_aim()
    expect(gone, "the block broke")
    expect(broken and broken.x == x and broken.y == y and broken.z == z and broken.block == block,
        "on_block_broken reported it")
    local item = (block == "grass") and "dirt" or block
    expect(count_item(item) >= 1, "the drop is in the bag")
end)

test("a right click places the held block on the face under the pointer", function()
    local x, y, z, block = find_block(2)
    expect(x ~= nil, "found a block to build on")
    if not x then return end
    -- Hold something placeable: the first slot, which the break test filled.
    give_item("stone", 1)
    press_key("1")
    wait(1)
    release_key("1")
    wait(1)
    expect(selected_slot() == 1, "the 1 key picks the first slot")
    press_mouse("right")
    wait(3)
    release_mouse("right")
    clear_aim()
    expect(placed ~= nil and placed.x == x and placed.y == y and placed.z == z + 1,
        "on_block_placed reported the cell above the face")
    expect(get_block(x, y, z + 1) ~= "air", "the block stands there")
end)
