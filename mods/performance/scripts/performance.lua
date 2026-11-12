-- Performance.
--
--   F4    the performance line (frames per second, frame time, chunks, faces), then the
--         line with the frame-time graph (240 frames of main-thread work; lines at 8.3
--         and 16.7 ms), then off
--   F6    step the quality level by hand; it stops adapting until F6 comes back round to "auto"
--
-- Quality levels trade detail for speed. Level 1 is everything on. Each step down cuts
-- work: a smaller margin of chunks kept beyond the screen, less time per frame spent
-- building meshes, then no cast shadows, then no corner shading. The level drops when the
-- frame rate stays low and climbs back when it stays high.

local LEVELS = {
    { name = "full",    shadows = true,  occlusion = true,  margin = 96, budget = 2.0,  horizon = 48 },
    { name = "lean",    shadows = true,  occlusion = true,  margin = 48, budget = 1.5,  horizon = 40 },
    { name = "fast",    shadows = false, occlusion = true,  margin = 32, budget = 1.0,  horizon = 32 },
    { name = "fastest", shadows = false, occlusion = false, margin = 16, budget = 0.75, horizon = 24 },
}
local LOW_FPS = 42          -- below this for LOW_SECONDS, step down
local HIGH_FPS = 56         -- above this for HIGH_SECONDS, step up
local LOW_SECONDS = 3
local HIGH_SECONDS = 12
local WARMUP_SECONDS = 6    -- the world is still streaming in; do not judge yet

local level = 1
local auto = true
local overlay = 0   -- 0 off, 1 line, 2 line and graph
local elapsed = 0
local low_for = 0
local high_for = 0
local since_line = 0

local function apply(n, why)
    level = math.max(1, math.min(#LEVELS, n))
    local q = LEVELS[level]
    set_setting("shadows", q.shadows)
    set_setting("occlusion", q.occlusion)
    set_setting("view_margin", q.margin)
    set_setting("mesh_budget_ms", q.budget)
    set_setting("horizon", q.horizon)
    if why then
        show_text("performance: " .. q.name .. " quality (" .. why .. ")", 3)
        log("performance: level " .. level .. " " .. q.name .. " (" .. why .. ")")
    end
end

function on_script_loaded()
    apply(1)
    log("performance: F4 shows the numbers, F6 sets quality by hand")
end

function on_tick(dt)
    elapsed = elapsed + dt
    local p = perf()

    if key_just_pressed("F4") then
        overlay = (overlay + 1) % 3
        show_perf_graph(overlay == 2)
        since_line = 1
    end
    if key_just_pressed("F6") then
        if auto then
            auto = false
            apply(1, "by hand")
        elseif level < #LEVELS then
            apply(level + 1, "by hand")
        else
            auto = true
            apply(1, "automatic again")
        end
        low_for, high_for = 0, 0
    end

    -- Adapt, but never while the window is in the background (the game idles there on
    -- purpose) or while the world is still loading.
    if auto and window_focused() and elapsed > WARMUP_SECONDS and p.fps > 0 then
        if p.fps < LOW_FPS then
            low_for = low_for + dt
            high_for = 0
        elseif p.fps > HIGH_FPS then
            high_for = high_for + dt
            low_for = 0
        else
            low_for, high_for = 0, 0
        end
        if low_for >= LOW_SECONDS and level < #LEVELS then
            apply(level + 1, string.format("%.0f frames per second", p.fps))
            low_for = 0
        elseif high_for >= HIGH_SECONDS and level > 1 then
            apply(level - 1, "running smoothly")
            high_for = 0
        end
    end

    if overlay > 0 then
        since_line = since_line + dt
        if since_line >= 0.25 then
            since_line = 0
            set_hud("performance", string.format(
                "%.0f fps  %.1f ms  work %.1f ms  %d spikes  %d chunks  %d faces  %.1f MB  %s%s",
                p.fps, p.frame_ms, p.work_ms, p.spikes, p.chunks, p.faces,
                p.mesh_bytes / 1048576, LEVELS[level].name,
                auto and "" or " (by hand)"))
        end
    else
        set_hud("performance", "")
    end
end
