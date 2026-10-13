-- The app installs this callback before loading device.lua.
return function(Device)
    if not KAIA_DEVICE then return end
    local function yes() return true end
    local function get(key) return KAIA_DEVICE(key) end
    local function set(key, value) return KAIA_DEVICE(key, value) end
    local power = {
        fl_min = 0, fl_max = 100, fl_warmth_min = 0, fl_warmth_max = 100,
        getCapacity = function() return get("battery") end,
        getCapacityHW = function() return get("battery") end,
        isCharging = function() return get("charging") end,
        isCharged = function() return get("charged") end,
        frontlightIntensity = function() return get("intensity") end,
        frontlightWarmth = function() return get("warmth") end,
        isFrontlightOn = function() return get("intensity") > 0 end,
        setIntensity = function(_, value) set("intensity", value) end,
        setWarmth = function(_, value) set("warmth", value) end,
        turnOffFrontlight = function() set("intensity", 0) end,
        turnOnFrontlight = function() set("intensity", 50) end,
        toggleFrontlight = function() set("intensity", get("intensity") > 0 and 0 or 50) end,
    }
    Device.powerd = power
    Device.getPowerDevice = function() return power end
    Device.hasBattery = yes
    Device.hasFrontlight = yes
    Device.hasNaturalLight = yes
    Device.hasNaturalLightMixer = yes
    Device.canSuspend = yes
    Device.suspend = function() set("suspend", true) end
    Device.isKobo = function() return get("kobo") end
    Device.isDesktop = function() return not get("kobo") end
end
