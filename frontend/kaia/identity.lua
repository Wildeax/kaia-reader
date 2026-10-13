--[[--
Kaia Reader product identity (KR-001).

Kaia installs and stores data separately from stock KOReader, so both can live
on the same device without sharing a writable profile or an update feed.

@module kaia.identity
]]

local Identity = {
    app_name = "Kaia Reader",
    -- Directory name used under XDG/Application Support paths and on Android.
    -- On Kobo the data directory is the install directory (.adds/kaia).
    data_dir_name = "kaia",
    version = "0.1.0-dev",
    upstream = "KOReader v2026.07.1",
    upstream_commit = "9192014d8bd82a91dc1012473be0f238dedfdb54",
    issues_url = "https://github.com/Wildeax/kaia-reader/issues",
    source_url = "https://github.com/Wildeax/kaia-reader",
    -- Kaia has no update channel yet. While this is nil, the KOReader OTA
    -- menu is hidden so Kaia never downloads KOReader packages over itself.
    update_channel = nil,
}

--- Returns "Kaia Reader 0.1.0-dev (KOReader v2026.07.1, git-rev)".
function Identity:versionString(git_rev)
    local s = string.format("%s %s (%s", self.app_name, self.version, self.upstream)
    if git_rev then
        s = s .. ", " .. git_rev
    end
    return s .. ")"
end

return Identity
