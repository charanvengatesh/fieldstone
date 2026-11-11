-- Light sources and the day clock.

local function top_of(x, y)
    for z = 31, 0, -1 do
        if get_block(x, y, z) ~= "air" then return z end
    end
    return nil
end

test("a torch lights the air around it, one level less per block", function()
    local px, py = player_pos()
    local x, y = math.floor(px) + 3, math.floor(py) + 3
    local z = top_of(x, y)
    expect(z ~= nil, "ground to stand a torch on")
    if not z then return end
    local before = light_at(x, y, z + 2)
    expect(before == 0, "dark before the torch: " .. before)
    expect(set_block(x, y, z + 1, "torch"), "placed the torch")
    expect(light_at(x, y, z + 1) == 14, "the torch cell is at 14: " .. light_at(x, y, z + 1))
    expect(light_at(x, y, z + 2) == 13, "one up is 13: " .. light_at(x, y, z + 2))
    expect(light_at(x + 2, y, z + 2) == 11, "two over and one up is 11: " .. light_at(x + 2, y, z + 2))
    expect(light_at(x, y, z) == 0, "the ground block itself carries none")
    set_block(x, y, z + 1, "air")
    expect(light_at(x, y, z + 2) == 0, "dark again once it is gone")
end)

test("the day clock reads back what it is set to and keeps running", function()
    local was = time_of_day()
    set_time_of_day(0.78)
    local t = time_of_day()
    expect(math.abs(t - 0.78) < 1e-3, "dusk: " .. t)
    wait(30)
    expect(time_of_day() > t, "time moves on")
    set_time_of_day(1.25)
    expect(math.abs(time_of_day() - 0.25) < 1e-3, "wraps around the day")
    set_time_of_day(was)
end)

test("a torch at night shows up in a snapshot", function()
    local px, py = player_pos()
    local x, y = math.floor(px) + 2, math.floor(py) + 1
    local z = top_of(x, y)
    if z then set_block(x, y, z + 1, "torch") end
    local was = time_of_day()
    set_time_of_day(0.0)
    wait(2)
    expect(snapshot("test_torch_night"), "snapshot written")
    set_time_of_day(was)
    if z then set_block(x, y, z + 1, "air") end
end)
