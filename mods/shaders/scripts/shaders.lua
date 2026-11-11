-- Shader presets. F7 cycles: plain, outline, crt. Plain is the default: the picture as
-- the engine draws it.
--
-- The passes themselves are in shaders.ron and shaders/*.wgsl; this only turns them on
-- and off. Add your own pass there and it joins the chain by order.

local PRESETS = {
    { name = "plain",   outline = false, crt = false },
    { name = "outline", outline = true,  crt = false },
    { name = "crt",     outline = true,  crt = true },
}
local current = 1

local function apply()
    local p = PRESETS[current]
    set_shader("outline", p.outline)
    set_shader("crt", p.crt)
    if current == 1 then
        set_hud("shaders", "")
    else
        set_hud("shaders", "look: " .. p.name .. " (F7)")
    end
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
