-- Small adapters for calls absent from the bundled plugin classes.
return function(module)
    if module.name == "docsettingtweak" and not module.loadDefaults then
        -- The editor calls this after saving. DocSettingsLoad reads the file
        -- again for each book, so there is no instance cache to update.
        function module:loadDefaults()
            return require("luasettings"):open(self.settings_file).data
        end
    end
end
