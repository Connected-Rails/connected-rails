-- AFB (automatische Fahr-/Bremssteuerung): holds the speed set in the cab.
--
-- Called once per frame for every train whose leading vehicle names this script.
-- `ctx` is read-only; the returned table is applied to the cab controls. Returning nil
-- leaves the driver in charge.
--
-- The module also draws the "mfa" cab screen through the `display` hook below;
-- for every other screen it returns nil, which hands the drawing back to the
-- widget list in the vehicle file.

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

-- ---------------------------------------------------------------------------
-- MFA screen: a small nested menu on the softkeys next to the display.
--
-- Softkey 1 (ctx.buttons[1]) cycles the pages FAHRT -> BREMSE. On FAHRT,
-- softkey 2 enters the EINHEIT submenu; inside it the keys change meaning
-- (1 toggles km/h <-> m/s, 2 goes back) — that is the nesting. State lives in
-- module locals, so it survives between frames.

local COL_TEXT = { 0.85, 0.9, 0.95 }
local COL_DIM = { 0.45, 0.5, 0.55 }
local COL_VALUE = { 1.0, 0.75, 0.2 }
local COL_LINE = { 0.3, 0.35, 0.4 }

local page = 1          -- 1 = FAHRT, 2 = BREMSE
local unit_menu = false -- inside the EINHEIT submenu (reached from FAHRT)
local unit_ms = false   -- big speed readout in m/s instead of km/h
local prev1, prev2 = false, false

-- "--" for a value the train protection does not deliver right now.
local function fmt(value, decimals, scale)
  if value == nil then
    return "--"
  end
  return string.format("%." .. decimals .. "f", value * (scale or 1.0))
end

-- One labelled value row: dim label left, amber value right-aligned.
local function value_row(cmds, y, label, text, unit)
  cmds[#cmds + 1] = { kind = "text", x = 12, y = y, text = label, size = 11, color = COL_DIM }
  cmds[#cmds + 1] = { kind = "text", x = 208, y = y, text = text, size = 11,
                      color = COL_VALUE, align = "right" }
  cmds[#cmds + 1] = { kind = "text", x = 214, y = y, text = unit, size = 11, color = COL_DIM }
end

function M.display(ctx)
  if ctx.display ~= "mfa" then
    return nil
  end

  -- Rising edges of the two softkeys; held state comes from the cab controls.
  local b1 = ctx.buttons[1] or false
  local b2 = ctx.buttons[2] or false
  local click1, click2 = b1 and not prev1, b2 and not prev2
  prev1, prev2 = b1, b2
  if unit_menu then
    if click1 then unit_ms = not unit_ms end
    if click2 then unit_menu = false end
  else
    if click1 then page = page % 2 + 1 end
    if click2 and page == 1 then unit_menu = true end
  end

  local vals = ctx.value or {}
  local cmds = {}
  local function add(c) cmds[#cmds + 1] = c end

  add({ kind = "clear", color = { 0.02, 0.05, 0.08 } })

  -- Header: device name left, current page (or submenu) right.
  local title = unit_menu and "EINHEIT" or (page == 1 and "FAHRT" or "BREMSE")
  add({ kind = "text", x = 8, y = 6, text = "MFA", size = 12, color = COL_DIM })
  add({ kind = "text", x = 248, y = 6, text = title, size = 12,
        color = COL_TEXT, align = "right" })
  add({ kind = "line", x1 = 4, y1 = 22, x2 = 252, y2 = 22, width = 1, color = COL_LINE })

  if unit_menu then
    -- Submenu: pick the unit of the big speed readout.
    local marks = { unit_ms and " " or ">", unit_ms and ">" or " " }
    add({ kind = "text", x = 24, y = 48, text = marks[1] .. " km/h", size = 14, color = COL_TEXT })
    add({ kind = "text", x = 24, y = 72, text = marks[2] .. " m/s", size = 14, color = COL_TEXT })
  elseif page == 1 then
    -- FAHRT: big speed, then what the AFB/LZB aims at. The mfa_* indicators
    -- are nil unless the LZB guides — shown as "--".
    local v = unit_ms and ctx.v_kmh / 3.6 or ctx.v_kmh
    add({ kind = "text", x = 128, y = 36, text = string.format("%.0f", v), size = 30,
          color = COL_TEXT, align = "center" })
    add({ kind = "text", x = 128, y = 68, text = unit_ms and "m/s" or "km/h", size = 10,
          color = COL_DIM, align = "center" })
    value_row(cmds, 88, "V-SOLL", fmt(vals.mfa_v_soll, 0), "km/h")
    value_row(cmds, 103, "V-ZIEL", fmt(vals.mfa_v_ziel, 0), "km/h")
    value_row(cmds, 118, "ZIELENTF.", fmt(vals.mfa_zielentfernung, 0), "m")
  else
    -- BREMSE: the three pressures, plus the brake pipe as a bar.
    value_row(cmds, 36, "HL", fmt(ctx.brake_pipe, 2), "bar")
    value_row(cmds, 54, "C", fmt(ctx.brake_cylinder, 2), "bar")
    value_row(cmds, 72, "HB", fmt(ctx.main_reservoir, 2), "bar")
    add({ kind = "rect", x = 12, y = 96, w = 200, h = 10, color = COL_LINE, filled = false })
    local fill = math.min(ctx.brake_pipe / 6.0, 1.0)
    if fill > 0 then
      add({ kind = "rect", x = 12, y = 96, w = 200 * fill, h = 10,
            color = { 0.3, 0.9, 0.4 }, filled = true })
    end
  end

  -- Footer: what the softkeys do right now.
  add({ kind = "line", x1 = 4, y1 = 136, x2 = 252, y2 = 136, width = 1, color = COL_LINE })
  local hint = unit_menu and "1 WECHSEL   2 ZURUECK"
    or (page == 1 and "1 SEITE   2 EINHEIT" or "1 SEITE")
  add({ kind = "text", x = 8, y = 142, text = hint, size = 10, color = COL_DIM })

  return cmds
end

return M
