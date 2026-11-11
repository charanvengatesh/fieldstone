-- Shader presets. F7 cycles: plain, palette, palette + outline, crt (all three).
--
-- The passes themselves are in shaders.ron and shaders/*.wgsl; this only turns them on
-- and off. Add your own pass there and it joins the chain by order.

local PRESETS = {
    { name = "plain",             palette = false, outline = false, crt = false },
    { name = "palette",           palette = true,  outline = false, crt = false },
    { name = "palette + outline", palette = true,  outline = true,  crt = false },
    { name = "crt",               palette = true,  outline = true,  crt = true },
}
local current = 3

local function apply()
    local p = PRESETS[current]
    set_shader("palette", p.palette)
    set_shader("outline", p.outline)
    set_shader("crt", p.crt)
    set_hud("shaders", "look: " .. p.name .. " (F7)")
end

function on_script_loaded()
    log("shaders: F7 cycles the look; passes in mods/shaders/shaders.ron")
    apply()
end

function on_tick(dt)
    if key_just_pressed("F7") then
        current = current % #PRESETS + 1
        apply()
    end
end
