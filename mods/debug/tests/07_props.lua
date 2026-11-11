-- Props, cutouts and water: what walks through what.

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

test("a prop never blocks the way and breaks at a touch", function()
    -- Which way D goes depends on the view: measure it on open ground.
    local px, py, pz = player_pos()
    press_key("D")
    wait(20)
    release_key("D")
    local mx, my = player_pos()
    local sx = (mx - px) > 0.05 and 1 or ((mx - px) < -0.05 and -1 or 0)
    local sy = (my - py) > 0.05 and 1 or ((my - py) < -0.05 and -1 or 0)
    teleport(px, py, pz)
    wait(5)
    px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    -- A flat run of three cells with a flower, a tuft and a torch standing in it.
    local props = { "flower", "tuft", "torch" }
    local cells = {}
    local changed = {}
    local function put(x, y, z, b)
        local was = get_block(x, y, z)
        if was ~= b then
            changed[#changed + 1] = { x, y, z, was }
            set_block(x, y, z, b)
        end
    end
    -- A level lane three cells wide along the walk: floor under it, nothing at body
    -- height beside it, so the only things in the way are the props. (Without the old
    -- automatic step-up any lip of terrain would stop the walk for its own reasons.)
    for i = 0, 3 do
        local x, y = cx + i * sx, cy + i * sy
        for dx = -1, 1 do
            for dy = -1, 1 do
                put(x + dx, y + dy, pz - 1, "stone")
                put(x + dx, y + dy, pz, "air")
                put(x + dx, y + dy, pz + 1, "air")
            end
        end
    end
    for i = 1, 3 do
        local x, y = cx + i * sx, cy + i * sy
        put(x, y, pz, props[i])
        cells[i] = { x, y }
    end
    expect(get_block(cells[3][1], cells[3][2], pz) == "torch", "the torch stands in the air cell")
    expect(light_at(cells[3][1], cells[3][2], pz) == 14, "and still shines")
    press_key("D")
    wait(60)
    release_key("D")
    local qx, qy = player_pos()
    local gone = math.sqrt((qx - px) ^ 2 + (qy - py) ^ 2)
    expect(gone > 2.5, string.format("walked right through them (%.2f cells)", gone))
    teleport(px, py, pz)
    for i = #changed, 1, -1 do
        local c = changed[i]
        set_block(c[1], c[2], c[3], c[4])
    end
end)

test("water is see-through for the mesher and solid for the walker", function()
    local px, py, pz = player_pos()
    local x, y = math.floor(px) + 3, math.floor(py) + 3
    local z = top_of(x, y)
    if not z then return end
    -- A pool one block deep in a hole; sand under it is drawn, so the world has faces
    -- there too, and the player cannot wade in.
    local was = get_block(x, y, z)
    set_block(x, y, z, "water")
    wait(3)
    expect(get_block(x, y, z) == "water", "the pool is there")
    teleport(x + 0.5, y + 0.5, z + 2)
    wait(40)
    local _, _, qz = player_pos()
    expect(qz >= z + 0.99, string.format("the player stands over the water, not in it (z %.2f)", qz))
    set_block(x, y, z, was)
    teleport(px, py, pz)
end)

test("a shore with leaves, glass and props shows up in a snapshot", function()
    local px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    set_block(cx + 2, cy, pz, "glass")
    set_block(cx + 2, cy, pz + 1, "glass")
    set_block(cx - 2, cy + 1, pz, "mushroom")
    wait(2)
    expect(snapshot("test_props"), "snapshot written")
    set_block(cx + 2, cy, pz, "air")
    set_block(cx + 2, cy, pz + 1, "air")
    set_block(cx - 2, cy + 1, pz, "air")
end)
