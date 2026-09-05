-- Lights one representative indication in each fitted housing.  Entry 15
-- alternates Zs 1 and Zs 8 to prove that both share one A-shaped unit while
-- retaining steady/flashing behaviour.
local M = {}

local active = {
  [0] = { "zs1" },
  [1] = { "zs1" },
  [2] = { "zs2_K" },
  [4] = { "zs3_4" },
  [6] = { "zs6" },
  [7] = { "zs7" },
  [8] = { "zs8" },
  [9] = { "zs8" },
  [12] = { "zs13" },
  [14] = { "zs1", "zs2_K" },
  [16] = { "zs2v_K" },
  [18] = { "zs3v_4" },
  [19] = { "zs2v_K" },
}

local function copy_lamps(source, base)
  local lamps = { base }
  if source then
    for _, lamp in ipairs(source) do
      lamps[#lamps + 1] = lamp
    end
  end
  return lamps
end

function M.aspect(ctx)
  local lamps = active[ctx.signal]
  if ctx.signal == 15 then
    lamps = (math.floor(ctx.time / 4.0) % 2 == 0) and { "zs1", "zs3_4" }
                                                    or { "zs8", "zs3_4" }
  end
  if ctx.main == "stop" then
    return { main = "stop", lamps = copy_lamps(lamps, "lamp_red") }
  end
  if ctx.distant == "expect_stop" then
    return { distant = "expect_stop", lamps = copy_lamps(lamps, "vr0_licht") }
  end
  return nil
end

return M
