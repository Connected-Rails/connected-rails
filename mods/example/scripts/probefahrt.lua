-- Scenario hook of `scenarios/probefahrt.ron` (plan 19.7).
--
-- The script decides *when*, the RON says *what*: it returns the names of events to fire,
-- and those events' actions run as if their trigger had come true.
--
-- This is the case a trigger table cannot cover: "the train has been standing for 60 s
-- after it had already been moving" needs memory.

local M = {}

local STALL_TIME = 60.0

local has_moved = false
local standing_since = nil

function M.on_load(ctx)
  return { message = "Test run loaded — " .. ctx.trains .. " train(s) on the line." }
end

function M.on_frame(ctx)
  if ctx.finished then
    return nil
  end
  local v = math.abs(ctx.v_kmh or 0.0)
  if v > 3.0 then
    has_moved = true
    standing_since = nil
    return nil
  end
  if not has_moved then
    return nil
  end
  if standing_since == nil then
    standing_since = ctx.time
    return nil
  end
  -- `fired` carries the events that have already gone off, so the hook does not have to
  -- keep track of that itself.
  if ctx.time - standing_since >= STALL_TIME and ctx.fired["stalled"] == nil then
    return { fire = { "stalled" } }
  end
  return nil
end

return M
