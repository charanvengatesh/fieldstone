-- The bench: a glide across chunk boundaries, two turns, a zoom out and back, and a
-- save, with the main thread's work per frame recorded. It prints one line per run, and
-- the gate is that no frame went past the spike line (25 ms: a debug build, headless).
-- `cargo run -- --bench` runs this file alone.

local function stats()
    local h = perf_history()
    local sum, worst, n = 0, 0, 0
    local sorted = {}
    for _, ms in ipairs(h) do
        if ms > 0 then
            n = n + 1
            sum = sum + ms
            worst = math.max(worst, ms)
            sorted[#sorted + 1] = ms
        end
    end
    table.sort(sorted)
    local p99 = sorted[math.max(1, math.floor(#sorted * 0.99))] or 0
    return n > 0 and sum / n or 0, p99, worst
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
    for i = 1, 240 do
        teleport(x + i * 0.1, y, z)
        wait(1)
    end
    local glide_avg, glide_p99, glide_worst = stats()
    local glide_spikes = perf().spikes
    -- Two quarter turns.
    for _ = 1, 2 do
        press_key("Q")
        wait(1)
        release_key("Q")
        wait(45)
    end
    -- Zoom out to the widest view and back.
    for _ = 1, 2 do
        press_key("Minus")
        wait(1)
        release_key("Minus")
        wait(20)
    end
    wait(60)
    for _ = 1, 2 do
        press_key("Equal")
        wait(1)
        release_key("Equal")
        wait(20)
    end
    -- A save.
    set_block(math.floor(x) + 2, math.floor(y) + 2, math.floor(z), "stone")
    press_key("F5")
    wait(1)
    release_key("F5")
    wait(10)
    local avg, p99, worst = stats()
    local p = perf()
    log(string.format(
        "BENCH glide: avg %.2f ms, p99 %.2f ms, worst %.2f ms, %d spikes | all: avg %.2f ms, p99 %.2f ms, worst %.2f ms, %d spikes | %d chunks, %d faces, %.1f MB of meshes",
        glide_avg, glide_p99, glide_worst, glide_spikes, avg, p99, worst, p.spikes,
        p.chunks, p.faces, p.mesh_bytes / 1048576))
    expect(p.chunks > 0, "the world is loaded")
    expect(p.spikes == 0, string.format("no frame's work went past %d ms (%d did)", p.spike_ms, p.spikes))
    set_setting("spike_ms", 12)
end)
