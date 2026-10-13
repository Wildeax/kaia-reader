describe("Kaia safe mode", function()
    local SafeMode, dir

    setup(function()
        require("commonrequire")
        SafeMode = require("kaia/safemode")
    end)

    before_each(function()
        dir = os.tmpname()
        os.remove(dir)
        require("libs/libkoreader-lfs").mkdir(dir)
    end)

    after_each(function()
        os.remove(dir .. "/kaia-startup-attempts")
        os.remove(dir .. "/safemode")
        os.remove(dir)
        SafeMode.active = false
        SafeMode.reason = nil
    end)

    local function noenv() return nil end

    it("stays off for normal startups and counts attempts", function()
        assert.is_false(SafeMode:init(dir, noenv))
        assert.are.equal(1, SafeMode.attempts)
        assert.is_false(SafeMode:init(dir, noenv))
        assert.are.equal(2, SafeMode.attempts)
    end)

    it("turns on after three startups that never became stable", function()
        for _ = 1, 3 do
            assert.is_false(SafeMode:init(dir, noenv))
        end
        assert.is_true(SafeMode:init(dir, noenv))
        assert.are.equal("failed_startups", SafeMode.reason)
    end)

    it("resets the counter once a run is stable", function()
        for _ = 1, 3 do SafeMode:init(dir, noenv) end
        SafeMode:markStable()
        assert.is_false(SafeMode:init(dir, noenv))
        assert.are.equal(1, SafeMode.attempts)
    end)

    it("turns on with the safemode flag file and clear() removes it", function()
        local f = io.open(dir .. "/safemode", "w")
        f:close()
        assert.is_true(SafeMode:init(dir, noenv))
        assert.are.equal("flag", SafeMode.reason)
        SafeMode:clear()
        assert.is_false(SafeMode:init(dir, noenv))
    end)

    it("turns on with KAIA_SAFE_MODE=1", function()
        assert.is_true(SafeMode:init(dir, function(k) return k == "KAIA_SAFE_MODE" and "1" or nil end))
        assert.are.equal("env", SafeMode.reason)
    end)

    it("skips only non-bundled plugins while active", function()
        local builtin = { coverbrowser = true }
        SafeMode.active = true
        assert.is_true(SafeMode:skipsPlugin("someplugin", builtin))
        assert.is_false(SafeMode:skipsPlugin("coverbrowser", builtin))
        SafeMode.active = false
        assert.is_false(SafeMode:skipsPlugin("someplugin", builtin))
    end)
end)
