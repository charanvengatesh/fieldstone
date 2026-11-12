-- The camera mod's controls, driven as a player would drive them.

local function settled()
    return not view_turning()
end

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

local function on_a_quarter()
    local a = view_angle() % 90
    return a < 0.5 or a > 89.5
end

test("Q turns a quarter smoothly and R comes back", function()
    local start = view_target()
    press_key("Q")
    wait(1)
    release_key("Q")
    wait(1)
    expect(near(view_target(), start + 90, 0.01), "Q asks for a quarter turn")
    expect(view_turning(), "the turn is a motion, not a cut")
    -- The world turns about the player: they stay put on screen all the way round.
    local px, py, pz = player_pos()
    local sx0, sy0 = screen_of(px, py, pz)
    local worst = 0
    for _ = 1, 120 do
        if settled() then break end
        local sx, sy = screen_of(px, py, pz)
        worst = math.max(worst, math.abs(sx - sx0), math.abs(sy - sy0))
        wait(1)
    end
    expect(worst < 2.0, string.format("the player stayed within %.1f texels of their spot mid-turn", worst))
    expect(wait_until(settled, 120), "the turn finishes")
    expect(near(view_angle(), view_target(), 0.01), "the angle lands on the target")
    press_key("R")
    wait(1)
    release_key("R")
    expect(wait_until(settled, 120), "the reset finishes")
    expect(near(view_angle() % 360, start % 360, 0.01),
        string.format("R returns to the starting view (%.1f vs %.1f)", view_angle() % 360, start % 360))
end)

test("Alt-drag orbits freely and letting go settles on a quarter", function()
    local start = view_angle()
    press_key("AltLeft")
    wait(1)
    for _ = 1, 10 do
        move_mouse(20, 0)
        wait(1)
    end
    wait(1)
    local turned = view_angle() - start
    expect(near(turned, 200 * 0.35, 1.0), string.format("200 px of drag turned %.1f degrees", turned))
    expect(target() == nil, "the pointer breaks nothing while dragging")
    release_key("AltLeft")
    wait(2)
    expect(wait_until(settled, 120), "the view settles after the drag")
    expect(on_a_quarter(), string.format("it settled on a quarter view (%.1f)", view_angle()))
end)

test("a vertical Alt-drag pitches the camera and P toggles perspective", function()
    local was = pitch()
    press_key("AltLeft")
    wait(1)
    for _ = 1, 8 do
        move_mouse(0, -10)
        wait(1)
    end
    release_key("AltLeft")
    wait(20)
    local now = pitch()
    expect(near(now, was + 80 * 0.25, 1.0), string.format("80 px of upward drag tipped the camera %.1f degrees", now - was))
    -- The player stays on screen whatever the pitch.
    local px, py, pz = player_pos()
    local sx, sy = screen_of(px, py, pz)
    -- Within the dead zone (20 texels at magnification 3) of the middle.
    expect(math.abs(sx) < 80 and math.abs(sy) < 60, string.format("the player is near the middle (%.0f, %.0f)", sx, sy))
    set_pitch(was)
    expect(get_setting("perspective") == false, "parallel by default")
    press_key("P")
    wait(1)
    release_key("P")
    wait(2)
    expect(get_setting("perspective") == true, "P switched to perspective")
    -- The block under the pointer is found in perspective too: the pad beside the player.
    local fx, fy, fz = player_pos()
    aim_at(math.floor(fx) + 1.5, math.floor(fy) + 0.5, math.floor(fz))
    wait(2)
    local t = target()
    expect(t ~= nil and t.z == math.floor(fz) - 1, "picking works in perspective")
    clear_aim()
    set_setting("perspective", false)
    wait_until(settled, 120)
end)

test("a trackpad twist orbits and rests onto a quarter", function()
    local start = view_angle()
    twist(0.5)
    wait(2)
    expect(near(view_angle() - start, math.deg(0.5), 1.0), "the twist turned the view")
    -- A quarter second with no twist, then the settling motion.
    wait(20)
    expect(wait_until(settled, 120), "the view settles after the twist")
    expect(on_a_quarter(), "it settled on a quarter view")
end)

test("Ctrl-scroll zooms and plain scroll does not", function()
    local z = zoom()
    local slot = selected_slot()
    press_key("ControlLeft")
    wait(1)
    scroll(1)
    wait(2)
    expect(zoom() == math.min(z + 1, 6), "Ctrl-scroll up zooms in")
    expect(selected_slot() == slot, "and leaves the hotbar alone")
    scroll(-1)
    wait(2)
    expect(zoom() == z, "Ctrl-scroll down zooms out")
    release_key("ControlLeft")
    wait(2)
    scroll(-1)
    wait(2)
    expect(zoom() == z, "plain scroll does not zoom")
    expect(selected_slot() ~= slot, "plain scroll moves the hotbar")
    scroll(1)
    wait(2)
end)

test("the compass names where the camera looks", function()
    local line = hud_text("camera")
    expect(line ~= nil and line:find("looking") ~= nil, "the camera line is on screen")
    press_key("C")
    wait(1)
    release_key("C")
    wait(2)
    expect(hud_text("camera") == nil, "C hides it")
    press_key("C")
    wait(1)
    release_key("C")
    wait(2)
    expect(hud_text("camera") ~= nil, "C shows it again")
end)

test("T cycles the tilt presets as a motion", function()
    expect(tilt() == "classic", "starts classic")
    press_key("T") wait(1) release_key("T") wait(1)
    expect(tilt() == "high", "T goes to high")
    expect(view_turning(), "the tilt is a motion")
    expect(wait_until(settled, 120), "and settles")
    for _ = 1, 3 do
        press_key("T") wait(1) release_key("T") wait(1)
    end
    expect(tilt() == "classic", "three more presses come round to classic")
    expect(wait_until(settled, 120), "and settle")
end)

test("Shift-drag pans and the pan drifts home when let go", function()
    local x0 = camera_pan()
    press_key("ShiftLeft")
    wait(1)
    for _ = 1, 6 do
        move_mouse(30, 0)
        wait(1)
    end
    wait(10)
    local x1 = camera_pan()
    expect(x1 - x0 > 50, string.format("the pan moved %.1f texels", x1 - x0))
    expect(target() == nil, "the pointer breaks nothing while panning")
    release_key("ShiftLeft")
    wait(420)
    local x2 = camera_pan()
    expect(math.abs(x2) < 12, string.format("the pan drifted home (%.1f)", x2))
end)

test("the camera leans toward the pointer", function()
    clear_aim()
    wait(60)
    -- The player's spot on the picture moves the other way as the camera leans right.
    local px, py, pz = player_pos()
    local sx0 = screen_of(px, py, pz)
    aim_screen(300, 0)
    wait(90)
    local sx1 = screen_of(px, py, pz)
    expect(sx0 - sx1 > 15, string.format("leaned %.1f texels toward the pointer", sx0 - sx1))
    clear_aim()
    wait(90)
    local sx2 = screen_of(px, py, pz)
    expect(math.abs(sx2 - sx0) < 3, "and came back")
end)

test("zooming toward the pointer keeps the block under it", function()
    local z = zoom()
    set_setting("peek", false)
    -- A stone of our own, two cells toward the viewer and above everything near it, so
    -- the pointer finds it whatever the terrain around the spawn looks like.
    local px, py, pz = player_pos()
    local cx, cy = math.floor(px), math.floor(py)
    local bx, by, best = cx, cy, math.huge
    for _, d in ipairs({ { 2, 0 }, { 0, 2 }, { -2, 0 }, { 0, -2 }, { 2, 2 }, { -2, -2 }, { 2, -2 }, { -2, 2 } }) do
        -- The cell that lands lowest on screen is the nearest to the viewer.
        local _, y = screen_of(cx + d[1] + 0.5, cy + d[2] + 0.5, pz)
        if y < best then bx, by, best = cx + d[1], cy + d[2], y end
    end
    local highest = math.floor(pz)
    for dx = -2, 2 do
        for dy = -2, 2 do
            local top = top_of(bx + dx, by + dy)
            if top then highest = math.max(highest, top) end
        end
    end
    local bz = math.min(highest + 1, math.floor(pz) + 3)
    local cleared = {}
    for h = bz, bz + 2 do
        local b = get_block(bx, by, h)
        if b ~= "air" then
            cleared[#cleared + 1] = { bx, by, h, b }
            set_block(bx, by, h, "air")
        end
    end
    set_block(bx, by, bz, "stone")
    wait(2)
    local sx, sy = screen_of(bx + 0.5, by + 0.5, bz + 1)
    aim_screen(sx, sy)
    wait(2)
    local before = target()
    zoom_at(6, sx, sy)
    -- Long enough for the 0.15 s zoom ease, short enough that the pan has not started
    -- drifting home (1.5 s), even on a slow machine.
    wait(20)
    -- The same screen pixel is now that many fewer texels from the middle.
    aim_screen(sx * z / 6, sy * z / 6)
    wait(2)
    local after = target()
    expect(before ~= nil and after ~= nil, string.format("something is under the pointer both times (%.0f,%.0f)", sx, sy))
    if before and after then
        expect(before.x == after.x and before.y == after.y and before.z == after.z,
            string.format("same block: %d,%d,%d vs %d,%d,%d", before.x, before.y, before.z, after.x, after.y, after.z))
    end
    clear_aim()
    set_zoom(z)
    set_setting("peek", true)
    set_block(bx, by, bz, "air")
    for _, c in ipairs(cleared) do set_block(c[1], c[2], c[3], c[4]) end
    wait(40)
end)
