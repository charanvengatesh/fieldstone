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
    local before = perf().chunks
    -- Over water there is nothing to land on, so move along the shore until it is dry.
    local loaded = false
    for _ = 1, 6 do
        teleport(x, y, 31)
        loaded = wait_until(function()
            return get_block(math.floor(x), math.floor(y), 0) ~= "air"
        end, 300)
        local top = top_of(math.floor(x), math.floor(y))
        if not loaded or (top and get_block(math.floor(x), math.floor(y), top) ~= "water") then
            break
        end
        x = x + 12
    end
    expect(loaded, "the chunk under the player loaded")
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
    -- Which way D goes depends on the view, so measure it first on open ground.
    local px, py, pz = player_pos()
    press_key("D")
    wait(20)
    release_key("D")
    local mx, my = player_pos()
    local dx, dy = mx - px, my - py
    expect(math.abs(dx) + math.abs(dy) > 0.3, "the player walks on D")
    local sx = dx > 0.05 and 1 or (dx < -0.05 and -1 or 0)
    local sy = dy > 0.05 and 1 or (dy < -0.05 and -1 or 0)
    teleport(px, py, pz)
    wait(5)
    px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    -- A two-high wall two cells along that way, three wide so a diagonal walk cannot
    -- slip round its corner; the cells up to it are cleared.
    local x1, y1 = cx + 2 * sx, cy + 2 * sy
    local wall = {}
    for dy = -1, 1 do
        for dx = -1, 1 do
            local wx, wy = x1 + dx, y1 + dy
            if math.abs(wx - cx) >= 2 or math.abs(wy - cy) >= 2 then
                for dz = 0, 1 do
                    if get_block(wx, wy, pz + dz) ~= "stone" then
                        wall[#wall + 1] = { wx, wy, pz + dz, get_block(wx, wy, pz + dz) }
                        set_block(wx, wy, pz + dz, "stone")
                    end
                end
            end
        end
    end
    set_block(cx + sx, cy + sy, pz, "air")
    set_block(cx + sx, cy + sy, pz + 1, "air")
    teleport(cx + 0.5, cy + 0.5, pz)
    wait(5)
    press_key("D")
    wait(90)
    release_key("D")
    wait(5)
    local qx, qy, qz = player_pos()
    expect(qz < pz + 0.5, string.format("a two-high wall was not climbed (z %.2f)", qz))
    expect(math.sqrt((qx - px) ^ 2 + (qy - py) ^ 2) > 0.5, "the player did move toward it")
    for _, b in ipairs(wall) do set_block(b[1], b[2], b[3], b[4]) end
end)
