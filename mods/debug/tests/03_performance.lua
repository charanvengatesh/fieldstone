-- The performance mod: its overlay, its hand-set levels, and the adaptive behaviour on a
-- pretend slow machine.

local function tap(key)
    press_key(key)
    wait(1)
    release_key(key)
    wait(1)
end

test("F4 shows the numbers, then the graph, then hides them", function()
    expect(hud_text("performance") == nil, "no line to begin with")
    tap("F4")
    wait(20)
    local line = hud_text("performance")
    expect(line ~= nil and line:find("fps") ~= nil, "the line shows frames per second")
    tap("F4")
    wait(20)
    expect(hud_text("performance") ~= nil, "the line stays while the graph shows")
    tap("F4")
    wait(2)
    expect(hud_text("performance") == nil, "a third F4 hides it")
end)

test("F6 steps the quality levels by hand and back to automatic", function()
    expect(get_setting("shadows") == true and get_setting("view_margin") == 96, "full quality to start")
    tap("F6")
    expect(get_setting("view_margin") == 96, "first press keeps level 1, by hand")
    tap("F6")
    expect(get_setting("view_margin") == 48, "level 2 narrows the margin")
    tap("F6")
    expect(get_setting("shadows") == false, "level 3 drops cast shadows")
    tap("F6")
    expect(get_setting("occlusion") == false, "level 4 drops corner shading")
    tap("F6")
    expect(get_setting("shadows") == true and get_setting("occlusion") == true
        and get_setting("view_margin") == 96, "one more press is automatic at full quality")
end)

test("a slow machine steps quality down, a fast one brings it back", function()
    set_time_scale(8)
    simulate_frame_ms(40)
    expect(wait_until(function() return get_setting("shadows") == false end, 500),
        "sustained 25 fps turned shadows off")
    expect(get_setting("view_margin") < 96, "and narrowed the margin")
    simulate_frame_ms(8)
    expect(wait_until(function()
        return get_setting("shadows") == true and get_setting("view_margin") == 96
    end, 900), "sustained 125 fps restored full quality")
    simulate_frame_ms(nil)
    set_time_scale(1)
end)

-- Compute-pool time to rebuild every loaded chunk at a quality level: the builds run off
-- the main thread, and perf().build_ms sums the ones that landed each frame.
local function rebuild_cost(shadows, occlusion)
    set_setting("mesh_budget_ms", 16)
    -- Changing either setting marks every chunk dirty.
    set_setting("shadows", shadows)
    set_setting("occlusion", occlusion)
    wait(1)
    local total, idle = 0, 0
    for _ = 1, 600 do
        local p = perf()
        total = total + p.build_ms
        if p.build_ms == 0 and p.inflight == 0 then idle = idle + 1 else idle = 0 end
        if idle >= 3 then break end
        wait(1)
    end
    return total
end

test("lower quality levels rebuild the world faster", function()
    local before = get_setting("shadows")
    -- Timings wobble; the best of two runs each is what the machine can do.
    local fastest = math.min(rebuild_cost(false, false), rebuild_cost(true, true) and rebuild_cost(false, false))
    local full = math.min(rebuild_cost(true, true), rebuild_cost(false, false) and rebuild_cost(true, true))
    log(string.format("rebuild of %d chunks: full %.1f ms, fastest %.1f ms", perf().chunks, full, fastest))
    expect(full > 0 and fastest > 0, "both rebuilds were measured")
    expect(fastest < full * 0.85, "no shadows and no corner shading is at least 15 percent cheaper")
    set_setting("mesh_budget_ms", 3)
    set_setting("shadows", before)
end)

-- Waits until nothing is generating or meshing for a few frames running, then reads the
-- numbers.
local function settled()
    local quiet = 0
    for _ = 1, 900 do
        local p = perf()
        if p.inflight == 0 and p.uploads == 0 then quiet = quiet + 1 else quiet = 0 end
        if quiet >= 5 then return p end
        wait(1)
    end
    return nil
end

test("the horizon bounds what the orthographic view loads", function()
    local horizon = get_setting("horizon")
    set_setting("perspective", false)
    -- Near first: a chunk once loaded is kept a little past the horizon, so raising it
    -- afterwards is what shows the difference.
    set_setting("horizon", 8)
    local near = settled()
    expect(near ~= nil, "the world settled at a horizon of 8 blocks")
    set_setting("horizon", 200)
    local far = settled()
    expect(far ~= nil, "the world settled at a horizon of 200 blocks")
    log(string.format("horizon 8: %d chunks, %d faces; horizon 200: %d chunks, %d faces",
        near.chunks, near.faces, far.chunks, far.faces))
    expect(near.chunks < far.chunks, "a near horizon loads fewer chunks")
    set_setting("horizon", horizon)
end)

test("orthographic and perspective views report their streaming cost", function()
    local perspective = get_setting("perspective")
    local function view(on)
        set_setting("perspective", on)
        local p = settled()
        expect(p ~= nil, "the world settled")
        -- The visible-set cache makes an ordinary frame's streaming pass cheap.
        local stream, worst = 0, 0
        for _ = 1, 30 do
            wait(1)
            local q = perf()
            stream = stream + q.stream_ms
            worst = math.max(worst, q.stream_ms)
        end
        return p, stream / 30, worst
    end
    local ortho, ortho_ms, ortho_worst = view(false)
    local persp, persp_ms, persp_worst = view(true)
    log(string.format(
        "orthographic: %d chunks, %d faces, stream %.3f ms (worst %.3f); perspective: %d chunks, %d faces, stream %.3f ms (worst %.3f)",
        ortho.chunks, ortho.faces, ortho_ms, ortho_worst, persp.chunks, persp.faces, persp_ms, persp_worst))
    expect(ortho.chunks > 0 and persp.chunks > 0, "both views loaded ground")
    expect(ortho_worst < 1.0, "a settled orthographic frame's streaming pass stays under a millisecond")
    set_setting("perspective", perspective)
end)
