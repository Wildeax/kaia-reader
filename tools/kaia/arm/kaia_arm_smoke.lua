require("setupkoenv")
local Store = require("kaia/store")
local RustLib = require("kaia/rustlib")
local lib, why = RustLib.load()
assert(lib, why)
print("abi", lib.kaia_abi_version())
os.execute("rm -rf /tmp/kaia-arm-store && mkdir -p /tmp/kaia-arm-store")
local s = assert(Store.open("/tmp/kaia-arm-store"))
local md5 = string.rep("a", 32)
local t0 = os.clock()
local put = assert(s:put{
    doc = { partial_md5 = md5, file_size = 1234, title = "Test" },
    anchor = { type = "reflowable", locator = "/body/DocFragment[3]/body/p[4]/text().12",
               quote = { exact = "It is a truth", prefix = "", suffix = " universally" },
               offset = { dx = 0, dy = -1.2 } },
    text = "first sticker", size = { w = 0.1, h = 0.07 }, rotation = 0, state = "attached",
    created_at = 0, updated_at = 0,
})
print("put id", put.id)
for i = 1, 200 do
    s:put{ doc = { partial_md5 = md5, file_size = 1234 }, anchor = { type = "fixed", page = i, x = 0.5, y = 0.5 },
           text = "s" .. i, size = { w = 0.1, h = 0.1 }, rotation = 0, state = "attached", created_at = 0, updated_at = 0 }
end
local list = assert(s:list(md5))
print("listed", #list, string.format("%.2fs for 201 puts and a list", os.clock() - t0))
print("changed", s:documentSeen(md5, 999, "Test"))
print("state after size change", s:get(put.id).state)
print("quote index", Store.quoteLocate("Chapter 1. It is a truth universally acknowledged", "It is a truth", "", " universally"))
local dir = assert(s:export())
print("export", dir)
s:close()
print("after close", s:list(md5))
print("OK")
