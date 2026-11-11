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
    local bx, by = x0 + 300, y0 - 120
    local before = perf().chunks
    -- Over water there is nothing to land on, so try spots around the target until one
    -- is dry under the whole body (it spans up to four columns; over water a column is
    -- closed from the surface to the sky and the fall never starts).
    local loaded, dry, x, y = false, false, bx, by
    for _, o in ipairs({ { 0, 0 }, { 16, 0 }, { 0, 16 }, { -16, 0 }, { 0, -16 }, { 32, 0 }, { 0, 32 },
        { -32, 0 }, { 0, -32 }, { 32, 32 }, { -32, -32 }, { 48, 0 } }) do
        x, y = bx + o[1], by + o[2]
        teleport(x, y, 31)
        loaded = wait_until(function()
            return get_block(math.floor(x), math.floor(y), 0) ~= "air"
        end, 300)
        if not loaded then break end
        dry = true
        for dx = -1, 1 do
            for dy = -1, 1 do
                local cx, cy = math.floor(x) + dx, math.floor(y) + dy
                local top = top_of(cx, cy)
                if not top or get_block(cx, cy, top) == "water" then dry = false end
            end
        end
        if dry then break end
    end
    expect(dry, "found dry ground near the target")
    expect(loaded, "the chunk under the player loaded")
    expect(wait_until(is_grounded, 600), "the player landed")
    local _, _, pz = player_pos()
    -- The body spans up to four columns and may have landed on any of them, a tree top
    -- included.
    local highest = 0
    for dx = -1, 1 do
        for dy = -1, 1 do
            highest = math.max(highest, top_of(math.floor(x) + dx, math.floor(y) + dy) or 0)
        end
    end
    expect(pz <= highest + 1.01 and pz > 0.99,
        string.format("the player stands on the ground (feet %.2f, highest top %d)", pz, highest))
    expect(perf().chunks >= 6 and perf().chunks <= 40,
        string.format("%d chunks around the new spot (was %d)", perf().chunks, before))
    -- Home again, so later tests start where they expect: the chunk back under the
    -- player, then the fall (a stale "grounded" from before the teleport does not count).
    teleport(x0, y0, 31)
    expect(wait_until(function()
        return get_block(math.floor(x0), math.floor(y0), 0) ~= "air"
    end, 300), "home chunk loaded again")
    local function fell()
        local _, _, z = player_pos()
        return z < 30.5
    end
    expect(wait_until(fell, 300), "the fall home started")
    expect(wait_until(is_grounded, 600), "the player landed back home")
end)

-- Nothing moves until the chunk under the player has streamed in; after a teleport that
-- can take a moment.
local function ground_loaded()
    local px, py = player_pos()
    return get_block(math.floor(px), math.floor(py), 0) ~= "air"
end

test("walking into a wall stops; a jump clears one block and not two", function()
    expect(wait_until(ground_loaded, 300), "the ground under the player is loaded")
    expect(wait_until(is_grounded, 120), "standing")
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
    -- A floor under the approach, so a dip in the terrain does not turn the step into a
    -- two-block climb.
    for _, c in ipairs({ { cx + sx, cy + sy }, { cx, cy } }) do
        if get_block(c[1], c[2], pz - 1) == "air" or get_block(c[1], c[2], pz - 1) == "water" then
            wall[#wall + 1] = { c[1], c[2], pz - 1, get_block(c[1], c[2], pz - 1) }
            set_block(c[1], c[2], pz - 1, "stone")
        end
    end
    teleport(cx + 0.5, cy + 0.5, pz)
    wait(5)
    press_key("D")
    wait(90)
    release_key("D")
    wait(5)
    local qx, qy, qz = player_pos()
    expect(qz < pz + 0.5, string.format("a two-high wall was not climbed (z %.2f)", qz))
    expect(math.sqrt((qx - px) ^ 2 + (qy - py) ^ 2) > 0.5, "the player did move toward it")
    -- Jumping at a two-high wall gets nowhere either.
    press_key("D")
    press_key("Space")
    wait(1)
    release_key("Space")
    wait(50)
    release_key("D")
    local _, _, jz = player_pos()
    expect(jz < pz + 0.5, string.format("nor jumped (z %.2f)", jz))
    -- Lower it to one block: walking stops short, a jump while walking steps up.
    for _, b in ipairs(wall) do
        if b[3] == pz + 1 then set_block(b[1], b[2], b[3], "air") end
    end
    teleport(cx + 0.5, cy + 0.5, pz)
    wait(5)
    press_key("D")
    wait(60)
    local _, _, wz = player_pos()
    expect(wz < pz + 0.5, string.format("one block is not climbed by walking alone (z %.2f)", wz))
    press_key("Space")
    wait(1)
    release_key("Space")
    -- Let go soon after, or the player walks over the step and off its far side.
    wait(14)
    release_key("D")
    expect(wait_until(is_grounded, 60), "landed after the jump")
    local _, _, uz = player_pos()
    expect(uz > pz + 0.9, string.format("a jump while walking takes the step (z %.2f)", uz))
    -- Off the step before the wall comes back, so nothing is built into the player.
    teleport(cx + 0.5, cy + 0.5, pz)
    wait(5)
    for _, b in ipairs(wall) do set_block(b[1], b[2], b[3], b[4]) end
end)

test("a jump rises and lands; in the air a second press does nothing", function()
    expect(wait_until(ground_loaded, 300), "the ground under the player is loaded")
    expect(wait_until(is_grounded, 120), "standing")
    local px, py, pz = player_pos()
    press_key("Space")
    wait(1)
    release_key("Space")
    local peak = pz
    for _ = 1, 60 do
        local _, _, z = player_pos()
        peak = math.max(peak, z)
        if _ == 8 then
            press_key("Space")
            wait(1)
            release_key("Space")
        else
            wait(1)
        end
    end
    expect(peak > pz + 1.0 and peak < pz + 1.6, string.format("the jump peaked %.2f blocks up", peak - pz))
    expect(wait_until(is_grounded, 60), "and landed")
    local _, _, z = player_pos()
    expect(near(z, pz, 0.01), "back where it started")
end)
