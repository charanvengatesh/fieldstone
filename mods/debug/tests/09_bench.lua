-- The bench: a glide across chunk boundaries, two turns, a zoom out and back, and a
-- save, with the main thread's work per frame recorded phase by phase. It prints one
-- line per run, and the gate is that no frame went past the spike line (25 ms: a debug
-- build, headless). `cargo run -- --bench` runs this file alone.

-- Runs `step(i)` then waits a frame, `frames` times, sampling the frame's work.
local function phase(frames, step)
    local sum, worst, n = 0, 0, 0
    local sorted = {}
    for i = 1, frames do
        if step then step(i) end
        wait(1)
        local ms = perf().work_ms
        sum = sum + ms
        worst = math.max(worst, ms)
        n = n + 1
        sorted[#sorted + 1] = ms
    end
    table.sort(sorted)
    local p99 = sorted[math.max(1, math.floor(#sorted * 0.99))] or 0
    return { avg = sum / n, p99 = p99, worst = worst }
end

local function tap(key)
    press_key(key)
    wait(1)
    release_key(key)
end

local function show(name, s)
    return string.format("%s avg %.2f p99 %.2f worst %.2f", name, s.avg, s.p99, s.worst)
end

test("bench: a glide, two turns, a zoom and a save stay under the spike line", function()
    set_setting("spike_ms", 25)
    set_setting("perspective", false)
    local x, y, z = player_pos()
    -- Let the arena settle, then start counting.
    wait(30)
    perf_reset()
    -- Glide east: a step a frame streams chunks in the way a walk does, without the
    -- terrain in the way.
    local glide = phase(240, function(i) teleport(x + i * 0.1, y, z) end)
    -- Two quarter turns, 45 frames each.
    local turns = phase(90, function(i) if i == 1 or i == 46 then tap("Q") end end)
    -- Zoom out two steps to the widest view, hold, and back.
    local zoom = phase(140, function(i)
        if i == 1 or i == 21 then tap("Minus") end
        if i == 101 or i == 121 then tap("Equal") end
    end)
    -- An edit and a save.
    local save = phase(20, function(i)
        if i == 1 then
            set_block(math.floor(x) + 2, math.floor(y) + 2, math.floor(z), "stone")
        elseif i == 3 then
            tap("F5")
        end
    end)
    local p = perf()
    log(string.format("BENCH %s | %s | %s | %s | %d spikes, %d chunks, %d faces, %.1f MB of meshes",
        show("glide", glide), show("turns", turns), show("zoom", zoom), show("save", save),
        p.spikes, p.chunks, p.faces, p.mesh_bytes / 1048576))
    expect(p.chunks > 0, "the world is loaded")
    expect(p.spikes == 0, string.format("no frame's work went past %d ms (%d did)", p.spike_ms, p.spikes))
    set_setting("spike_ms", 12)
end)
