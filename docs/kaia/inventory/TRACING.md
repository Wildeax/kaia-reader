# Runtime compatibility tracing

Tracing records what plugins and core code do at run time: which modules load, which widgets open and close, timers, events, settings keys, menu registrations and file opens. It complements the static scan in `tools/kaia/inventory/plugin_inventory.py`, which reads source text and cannot see dynamic Lua behavior.

The tracer is `frontend/kaia/trace.lua` (KR-012). It is off unless the `KAIA_TRACE` environment variable is set.

## Run a trace

Start the desktop emulator with tracing on:

```sh
source tools/kaia/env.sh
KAIA_TRACE=/tmp/trace.jsonl ./kodev run
```

Run the workflow you want to observe: enable a plugin, open a book, change a setting, suspend and resume, then quit normally. Records are written every 50 entries. The last partial batch is written when the app calls `os.exit` at the end of `reader.lua`, so a normal quit keeps it. A process killed with SIGKILL loses up to 49 records. The file is truncated at each start.

Summarize the trace:

```sh
python3 -I tools/kaia/inventory/trace_summary.py /tmp/trace.jsonl > /tmp/trace-summary.md
```

The script uses the standard library only and prints Markdown to stdout.

## Privacy rule

The trace never contains book text, file names, setting values or widget text. Tests in `spec/unit/kaia_trace_spec.lua` check this with secret strings. The tracer records only:

- module names passed to `require`, first load only;
- widget class names: `widget.name` when it is a plain identifier, otherwise the source path of the widget's `init` method;
- event handler names such as `onCloseDocument`, never event arguments;
- timer delays for `scheduleIn`, `nextTick` and `tickAfterNext` (the last two record 0);
- setting key names for `LuaSettings` and `DocSettings`, never values;
- file open modes, the file extension, and whether the path is in the plugins directory, the data directory or elsewhere. The file name and directory names are not recorded;
- main menu registrations, by plugin.

Keys, handlers and module names that do not match a strict identifier pattern are written as `other`. A widget name that is not an identifier falls back to the source path of its `init` method, or `other`. Do not add fields that hold user content.

## Record format

Each line is one JSON object:

```json
{"t":1791870483.341807,"plugin":"core","detail":{"module":"random"},"kind":"require"}
```

| Field | Meaning |
|---|---|
| `t` | Seconds since the epoch as a float (`ffi/util` `gettime`) |
| `kind` | `require`, `widget_show`, `widget_close`, `timer`, `event`, `setting`, `file_open`, `menu_register` |
| `plugin` | Plugin directory name (`plugins/<name>.koplugin/`) found on the call stack, or `core` |
| `detail` | Kind-specific fields listed above |

Plugin attribution walks up to eight stack frames from the call. A timer that runs later from the UI loop has no plugin frame, so its callback is not attributed. The record that schedules it is attributed.

## Read the summary

The report has two parts.

- **Totals by plugin** gives one row per plugin with counts: distinct modules, widgets shown, timers, events, distinct settings keys, menu registrations and file opens.
- **Details by plugin** lists the names behind each count, with occurrence counts. Read the modules list to see what a plugin pulled in. Read the file access lines to see whether a plugin reads files outside the plugins and data directories.

Malformed lines are counted and skipped. A report with a nonzero skipped count points to a truncated trace, usually from a crash.

## Combine with the static inventory

The static scan and the trace answer different questions:

- The static inventory (`plugin_inventory.py`) lists what the source can reach: `require` calls, global access and menu code. It finds paths the workflow did not run.
- The trace lists what ran during one workflow. It finds dynamic loads and calls the scan cannot see.

Compare them per plugin. A module in the static list but absent from the trace was not exercised by that workflow. A module in the trace but absent from the static list was loaded dynamically. Record each such case in the compatibility profile as supported, classified or unsupported. Do not treat a clean trace as proof that every path works.

## Limitations

- Modules loaded before the hook in `reader.lua` (startup helpers such as `luasettings` and `datastorage`) are not recorded.
- `require` records only the first load of each module name, and only when the module was not already loaded at install time.
- `widget_show` and `widget_close` use the source path of `init` as the class label. A subclass without its own `init` gets the path of the nearest ancestor that defines one.
- Code that captures a function before install (for example `local show = UIManager.show`) bypasses the wrapper.
- A call in tail position (`return require("x")`) removes the caller's stack frame, so the record is attributed to `core`. Plugins that call `require` as a statement are attributed correctly.
- Records are buffered in memory. A crash or SIGKILL loses up to 49 records.
- Event traces can be large. Filter by `kind` or `plugin` with `jq` or a script if the summary is too coarse.
- The fallback JSON encoder is used only when `rapidjson` is missing. The desktop build includes `rapidjson`.
