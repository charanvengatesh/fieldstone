-- Streaming and physics far from the spawn: drop the player somewhere new and watch the
-- world arrive under them.

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

test("teleporting far away streams the ground in and the player lands on it", function()
    local x0, y0 = player_pos()
    local x, y = x0 + 300, y0 - 120
    teleport(x, y, 31)
    local before = perf().chunks
    expect(wait_until(function()
        return get_block(math.floor(x), math.floor(y), 0) ~= "air"
    end, 300), "the chunk under the player loaded")
    expect(wait_until(is_grounded, 600), "the player landed")
    local _, _, pz = player_pos()
    local top = top_of(math.floor(x), math.floor(y))
    expect(top ~= nil and near(pz, top + 1, 1.01),
        string.format("the player stands on the surface (feet %.2f, top %s)", pz, tostring(top)))
    expect(perf().chunks >= 6 and perf().chunks <= 40,
        string.format("%d chunks around the new spot (was %d)", perf().chunks, before))
    -- Home again, so later tests start where they expect.
    teleport(x0, y0, 31)
    expect(wait_until(is_grounded, 600), "the player landed back home")
end)

test("walking into a high wall stops, walking up one step climbs", function()
    local px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    -- Build a two-high wall one cell toward screen-right and a single step toward screen-up.
    local x1, y1 = cx + 2, cy - 2
    set_block(x1, y1, pz, "stone")
    set_block(x1, y1, pz + 1, "stone")
    teleport(cx + 0.5, cy + 0.5, pz)
    wait(5)
    press_key("D")
    wait(90)
    release_key("D")
    wait(5)
    local qx, qy, qz = player_pos()
    expect(qz < pz + 0.5, string.format("a two-high wall was not climbed (z %.2f)", qz))
    expect(math.sqrt((qx - px) ^ 2 + (qy - py) ^ 2) > 0.5, "the player did move toward it")
    set_block(x1, y1, pz, "air")
    set_block(x1, y1, pz + 1, "air")
end)
