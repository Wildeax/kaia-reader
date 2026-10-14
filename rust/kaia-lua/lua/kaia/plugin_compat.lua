-- Small adapters for calls absent from the bundled plugin classes.
return function(module)
    if module.name == "pencil" then require("kaia/pencil")(module) end
    if module.name == "docsettingtweak" and not module.loadDefaults then
        -- The editor calls this after saving. DocSettingsLoad reads the file
        -- again for each book, so there is no instance cache to update.
        function module:loadDefaults()
            return require("luasettings"):open(self.settings_file).data
        end
    end
    if module.name == "highlightsync" then
        function module:SyncBookHighlights()
            require("ui/uimanager"):show(require("ui/widget/infomessage"):new{
                text = require("gettext")("HighlightSync supports local merges only in Kaia."),
            })
        end
        local on_sync = module.onSync
        function module:onSync(local_path, cached_path, income_path)
            -- The host already has the open document. A reload would discard it.
            local result = on_sync(self, local_path, cached_path, income_path, false)
            self.ui.annotation:replaceItems(self.ui.annotation.annotations)
            DataAnnotations = self.ui.annotation.annotations
            return result
        end
    end
    if module.name == "anki" then
        -- The pinned plugin probes AnkiConnect before choosing its offline queue.
        -- Kaia supports the queue only, so it must never send that probe.
        local connect = require("ankiconnect")
        function connect:is_running() return false, require("gettext")("Offline in Kaia.") end
    end

    if module.name == "footcream" then
        local init, scan, ready = module.init, module._startFastScan, module.onReaderReady
        function module:onReaderReady()
            self._paintTo_wrapped = true
            ready(self)
            self.view:registerViewModule("footcream", self)
        end
        function module:paintTo(bb) self:_drawHighlights(bb) end
        function module:init()
            G_reader_settings:saveSetting("footcream_tap_mode", 1)
            init(self)
        end
        function module:_startFastScan(doc)
            -- Fork cannot use a document owned by a Rust worker thread.
            -- Use the pinned scanner's synchronous fallback on the host thread.
            local ffiutil = require("ffi/util")
            local fork = ffiutil.runInSubProcess
            ffiutil.runInSubProcess = nil
            local ok, result = pcall(scan, self, doc)
            ffiutil.runInSubProcess = fork
            if not ok then error(result, 0) end
            return result
        end
        local function unavailable()
            require("ui/uimanager"):show(require("ui/widget/infomessage"):new{
                text = require("gettext")("Kaia supports Footcream underlines only. Book files stay unchanged."),
            })
        end
        module._applyMetricEdition = unavailable
        module._doApplyMetricEdition = unavailable
        module._revertMetricEdition = unavailable
        module._showModeDialog = unavailable
        module._flushReports = function() end
        module._checkForUpdate = unavailable
        module._installUpdate = unavailable
        module._installUpdateSync = unavailable
    end

end
