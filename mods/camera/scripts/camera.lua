-- Camera control.
--
--   Hold Alt (or the middle mouse button) and drag              orbit: sideways turns, up and down pitches
--   Trackpad rotate gesture (two fingers, twist)               orbit freely
--   Let go                                                     settle on the nearest quarter view
--   Hold Shift and move the pointer                             pan away from the player; let go to drift back
--   T                                                          next tilt preset (engine key)
--   R                                                          back to the view the game started with
--   Ctrl or Cmd + scroll                                       zoom
--   P                                                          perspective on or off
--   C                                                          show or hide the compass
--
-- Turning costs nothing: chunk meshes are in world space and only the camera moves.

local DRAG_DEGREES_PER_PIXEL = 0.35   -- how fast a drag turns the view
local DRAG_PITCH_PER_PIXEL = 0.25     -- how fast a vertical drag tips it
local PAN_TEXELS_PER_PIXEL = 0.5      -- how far a Shift-drag pans
local SNAP_DEGREES = 90               -- views the camera settles on; try 45 for eight views
local GESTURE_REST = 0.25             -- seconds without a twist before the view settles
local ZOOM_STEP = 1.0                 -- scroll lines per zoom level

local orbiting = false       -- a drag is in progress
local panning = false        -- a Shift-drag is in progress
local twisting = false       -- a trackpad twist is in progress
local since_twist = 0
local zoom_scroll = 0
local compass = true
local compass_line = nil   -- what the HUD shows, so it is written only when it changes
local home = 0              -- the view the game started with; R comes back to it

local NAMES = { "south-west", "south", "south-east", "east", "north-east", "north", "north-west", "west" }

-- Which way the camera looks, with +y as north and +x as east.
local function facing(degrees)
    local i = math.floor(degrees / 45 + 0.5) % 8
    return NAMES[i + 1]
end

function on_script_loaded()
    home = view_target()
    log("camera: Alt-drag or twist to orbit (up and down pitches), R resets, Ctrl/Cmd-scroll zooms, P perspective, C compass")
end

function on_tick(dt)
    -- Drag to orbit. While it lasts the pointer breaks and places nothing.
    local held = key_pressed("AltLeft") or key_pressed("AltRight") or mouse_pressed("middle")
    if held then
        if not orbiting then
            orbiting = true
            capture_pointer(true)
        end
        local dx, dy = mouse_delta()
        if dx ~= 0 then
            set_view_angle(view_angle() + dx * DRAG_DEGREES_PER_PIXEL)
        end
        if dy ~= 0 then
            -- Dragging up looks more from above.
            set_pitch(pitch() - dy * DRAG_PITCH_PER_PIXEL)
        end
    elseif orbiting then
        orbiting = false
        capture_pointer(false)
        snap_view(SNAP_DEGREES)
    end

    -- Shift-drag to pan. The pan is held while Shift is down and drifts home after.
    if key_pressed("ShiftLeft") or key_pressed("ShiftRight") then
        if not panning then
            panning = true
            capture_pointer(true)
        end
        local dx, dy = mouse_delta()
        pan_view(dx * PAN_TEXELS_PER_PIXEL, -dy * PAN_TEXELS_PER_PIXEL)
    elseif panning then
        panning = false
        if not orbiting then capture_pointer(false) end
    end

    -- Twist to orbit. A gesture has no "released" signal, so settle after a short rest.
    local twist = rotate_gesture()
    if twist ~= 0 then
        twisting = true
        since_twist = 0
        set_view_angle(view_angle() + math.deg(twist))
    elseif twisting then
        since_twist = since_twist + dt
        if since_twist >= GESTURE_REST then
            twisting = false
            snap_view(SNAP_DEGREES)
        end
    end

    if key_just_pressed("R") then
        -- Turn the short way round to the starting view.
        local off = (view_target() - home) % 360
        if off > 180 then off = off - 360 end
        turn_view(-off)
    end

    -- Zoom on a modified scroll; plain scrolling stays with the hotbar.
    if key_pressed("ControlLeft") or key_pressed("ControlRight")
        or key_pressed("SuperLeft") or key_pressed("SuperRight") then
        zoom_scroll = zoom_scroll + scroll_delta()
        while zoom_scroll >= ZOOM_STEP do
            set_zoom(zoom() + 1)
            zoom_scroll = zoom_scroll - ZOOM_STEP
        end
        while zoom_scroll <= -ZOOM_STEP do
            set_zoom(zoom() - 1)
            zoom_scroll = zoom_scroll + ZOOM_STEP
        end
    else
        zoom_scroll = 0
    end

    if key_just_pressed("P") then
        set_setting("perspective", not get_setting("perspective"))
    end
    if key_just_pressed("C") then
        compass = not compass
    end
    if compass then
        local how = get_setting("perspective") and "perspective" or "parallel"
        local line = string.format("looking %s, %s tilt (%.0f degrees), %s", facing(view_angle()), tilt(), pitch(), how)
        if line ~= compass_line then
            compass_line = line
            set_hud("camera", line)
        end
    elseif compass_line then
        compass_line = nil
        set_hud("camera", "")
    end
end
