--[[--
Kaia defaults that differ from stock KOReader. Each is applied only when the
user has never set the value, so a user's choice always wins.

@module kaia.defaults
]]

local Defaults = {
    -- Keep per-book data in Kaia's own data folder instead of a .sdr folder
    -- next to the book. Stock KOReader on the same device keeps writing its
    -- own .sdr folder, and Kaia never deletes it (see DocSettings:flush).
    -- Kaia still reads an existing KOReader .sdr the first time it opens a
    -- book, so highlights carry over.
    document_metadata_folder = "dir",
}

--- Applies the defaults to a LuaSettings object.
function Defaults:apply(settings)
    for key, value in pairs(self) do
        if type(value) ~= "function" and not settings:has(key) then
            settings:saveSetting(key, value)
        end
    end
end

return Defaults
