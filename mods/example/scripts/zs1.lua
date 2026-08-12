-- Zs1 (substitute signal) for signals of type `example:ks_main_zs1`.
--
-- Runs after the rule table and sees its result in `ctx.main`. Returning nil keeps that
-- result. This is the case a table cannot cover: it needs memory — how long has the signal
-- been at stop?
--
-- In reality the dispatcher gives Zs1 when a signal is defective; here it comes
-- automatically after three minutes, which makes the hook visible without a dispatcher UI.

local M = {}

local DELAY = 180.0
local at_stop_since = {}

function M.aspect(ctx)
  if ctx.main ~= "stop" then
    at_stop_since[ctx.signal] = nil
    return nil
  end
  local since = at_stop_since[ctx.signal]
  if since == nil then
    at_stop_since[ctx.signal] = ctx.time
    return nil
  end
  if ctx.time - since >= DELAY then
    -- Pass at walking pace: 40 km/h, driving on sight.
    return { main = "substitute", speed = 40.0, lamps = { "red", "zs1" } }
  end
  return nil
end

return M
