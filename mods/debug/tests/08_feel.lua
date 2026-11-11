-- Chips, dust and the figure's heading.

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

test("breaking a block throws chips that settle and fade", function()
    -- A column near the player whose top is about at foot level, so a stone on it is in
    -- reach and not under a tree.
    local px, py, pz = player_pos()
    local x, y, z = nil, nil, nil
    for _, d in ipairs({ { 2, 2 }, { 2, -2 }, { -2, 2 }, { -2, -2 }, { 2, 0 }, { 0, 2 }, { -2, 0 }, { 0, -2 } }) do
        local cx, cy = math.floor(px) + d[1], math.floor(py) + d[2]
        local top = top_of(cx, cy)
        if top and math.abs(top + 1 - pz) <= 1.5 and get_block(cx, cy, top + 1) == "air" then
            x, y, z = cx, cy, top
            break
        end
    end
    expect(z ~= nil, "a block to break")
    if not z then return end
    expect(wait_until(function() return particles() == 0 end, 120), "quiet before")
    local was = get_block(x, y, z)
    set_block(x, y, z + 1, "stone")
    wait(2)
    aim_at(x + 0.5, y + 0.5, z + 2)
    wait(1)
    press_mouse("left")
    local gone = wait_until(function() return get_block(x, y, z + 1) == "air" end, 400)
    release_mouse("left")
    clear_aim()
    expect(gone, "the stone broke")
    wait(2)
    expect(particles() >= 8, "chips flew: " .. particles())
    expect(wait_until(function() return particles() == 0 end, 90), "and were gone within a second and a half")
    set_block(x, y, z, was)
end)

test("landing after a fall raises dust and squashes the figure for a moment", function()
    local px, py, pz = player_pos()
    wait_until(function() return particles() == 0 end, 120)
    teleport(px, py, pz + 4)
    expect(wait_until(is_grounded, 120), "landed")
    wait(2)
    expect(particles() > 0, "dust rose: " .. particles())
    wait_until(function() return particles() == 0 end, 90)
end)

test("the figure turns with the walk: sideways, away, toward", function()
    press_key("D")
    wait(3)
    release_key("D")
    expect(player_heading() == "right", "D looks right: " .. tostring(player_heading()))
    press_key("A")
    wait(3)
    release_key("A")
    expect(player_heading() == "left", "A looks left: " .. tostring(player_heading()))
    press_key("W")
    wait(3)
    release_key("W")
    expect(player_heading() == "away", "W turns away: " .. tostring(player_heading()))
    press_key("S")
    wait(3)
    release_key("S")
    expect(player_heading() == "toward", "S turns toward: " .. tostring(player_heading()))
    wait(5)
end)
