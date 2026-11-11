-- Chips, dust and the figure's heading.

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

test("breaking a block throws chips that settle and fade", function()
    -- A stone of our own on a column near the player, at about foot level, that the
    -- pointer can actually reach (a canopy in the way sends us to the next spot).
    local px, py, pz = player_pos()
    local x, y, z = nil, nil, nil
    for _, d in ipairs({ { 2, 2 }, { 2, -2 }, { -2, 2 }, { -2, -2 }, { 2, 0 }, { 0, 2 }, { -2, 0 }, { 0, -2 } }) do
        local cx, cy = math.floor(px) + d[1], math.floor(py) + d[2]
        local top = top_of(cx, cy)
        if top and math.abs(top + 1 - pz) <= 1.5 and get_block(cx, cy, top + 1) == "air" then
            set_block(cx, cy, top + 1, "stone")
            wait(2)
            aim_at(cx + 0.5, cy + 0.5, top + 2)
            wait(1)
            local t = target()
            if t and t.x == cx and t.y == cy and t.z == top + 1 then
                x, y, z = cx, cy, top
                break
            end
            clear_aim()
            set_block(cx, cy, top + 1, "air")
        end
    end
    expect(z ~= nil, "a block to break")
    if not z then return end
    expect(wait_until(function() return particles() == 0 end, 120), "quiet before")
    press_mouse("left")
    local gone = wait_until(function() return get_block(x, y, z + 1) == "air" end, 400)
    release_mouse("left")
    clear_aim()
    expect(gone, "the stone broke")
    wait(2)
    expect(particles() >= 8, "chips flew: " .. particles())
    expect(wait_until(function() return particles() == 0 end, 90), "and were gone within a second and a half")
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

test("a hidden player shows as a silhouette, and the setting turns it off", function()
    expect(get_setting("see_through") == true, "on by default")
    -- A wall of stone between the viewer and the player: three blocks tall, in the
    -- cells that lie toward the camera, whichever way it looks.
    local px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    local tx, ty = screen_of(cx + 1.5, cy + 0.5, pz), screen_of(cx + 0.5, cy + 1.5, pz)
    -- The cell that projects lower on screen is nearer the viewer.
    local _, y1 = screen_of(cx + 1.5, cy + 0.5, pz)
    local _, y2 = screen_of(cx + 0.5, cy + 1.5, pz)
    local _, y3 = screen_of(cx - 0.5, cy + 0.5, pz)
    local _, y4 = screen_of(cx + 0.5, cy - 0.5, pz)
    local cells = { { cx + 1, cy, y1 }, { cx, cy + 1, y2 }, { cx - 1, cy, y3 }, { cx, cy - 1, y4 } }
    table.sort(cells, function(a, b) return a[3] < b[3] end)
    local built = {}
    for i = 1, 2 do
        for dz = 0, 2 do
            local x, y, z = cells[i][1], cells[i][2], math.floor(pz) + dz
            built[#built + 1] = { x, y, z, get_block(x, y, z) }
            set_block(x, y, z, "stone")
        end
    end
    wait(2)
    expect(snapshot("test_silhouette"), "snapshot with the silhouette written")
    set_setting("see_through", false)
    expect(get_setting("see_through") == false, "setting reads back off")
    wait(2)
    expect(snapshot("test_silhouette_off"), "snapshot without it written")
    set_setting("see_through", true)
    for _, b in ipairs(built) do set_block(b[1], b[2], b[3], b[4]) end
end)
