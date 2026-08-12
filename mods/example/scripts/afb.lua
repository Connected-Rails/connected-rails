-- AFB (automatische Fahr-/Bremssteuerung): holds the speed set in the cab.
--
-- Called once per frame for every train whose leading vehicle names this script.
-- `ctx` is read-only; the returned table is applied to the cab controls. Returning nil
-- leaves the driver in charge.

local M = {}

-- Proportional band [km/h]: full power 10 km/h below the target, full electric brake
-- 10 km/h above it.
local BAND = 10.0

function M.update(ctx)
  if not ctx.afb or ctx.reverser == 0 then
    return nil
  end
  -- The line speed always wins over the dial.
  local target = math.min(ctx.afb_target, ctx.speed_limit_kmh)
  local delta = target - ctx.v_kmh
  local notch = delta / BAND
  if notch > 1.0 then notch = 1.0 end
  if notch < -1.0 then notch = -1.0 end
  return { throttle = notch }
end

return M
