# kaia-app

The app shell of the Rust Kaia Reader: the main loop, the stack of screens, the
services, and the `kaia` binary. The screens are surfaces, one folder each. The
shell knows no surface. For the plan, see `docs/kaia/REWRITE_PLAN.md`. For the
toolkit and the screens, see `docs/kaia/specs/UI_TOOLKIT.md` and
`docs/kaia/specs/APP_UX.md`.

## Architecture

- **Loop** (`src/app.rs`). `App` runs the loop from `UI_TOOLKIT.md`: input, gestures,
  actions and events, paint, due tasks, paint again. It asks the router and the
  surfaces for windows and applies what they return.
- **Router** (`src/router.rs`). Keeps the stack of screens. The root is Home,
  Library, Notes or Settings. Pushed screens (reader, overlay, book details,
  settings sections) sit above it. Each screen holds one `Surface`, one window and
  its options. The router does not call surface hooks. The app does.
- **Surfaces** (`src/surface.rs`, `src/surfaces/`). A surface owns its state and
  builds its widget tree. It answers five calls: `route`, `build`, `on_command`,
  `on_event`, and the `on_show` and `on_hide` hooks. It returns an `Outcome`:
  `Handled`, `NotMine`, `Rebuild`, or `Navigate(Nav)` with `Push`, `Pop`,
  `SwitchRoot`, `Replace`.
- **Services** (`src/services.rs`). The platform, settings, library index, paths,
  theme and assets. A surface reaches them through `SurfaceEnv`.
- **Env** (`SurfaceEnv`, `src/surface.rs`). What a surface can use while it works:
  `services`, `ctx` (scheduler and refresh queue), `screen`, `notice`,
  `schedule_in`, `refresh`, `toast`, `dialog`, `sheet`, `show`.
- **Commands** (`src/command.rs` and each surface's `commands.rs`). Widgets send
  action ids. See the naming rule below.

### Routing of a command

1. The top surface gets `on_command(id, args, env)`.
2. If it answers `NotMine` and the root is another screen, the root gets it.
3. If both answer `NotMine`, the global commands run (`Command::parse`).
4. An id that nobody knows is recorded. `App::unknown_commands` lists them.

A surface's `Handled` stops routing. `Navigate` acts on the stack, not on the
surface that asked.

Events work differently. `App::broadcast_app` gives an app event (for example
`LibraryUpdated`) to the windows and to every surface, bottom up. A surface that
returns `Rebuild` is rebuilt at once when it is on top. A covered one is rebuilt
when it becomes the top screen again.

### Hooks

- `on_show`: the screen became the top of the stack (pushed, revealed or made root).
- `on_hide`: the screen is leaving the stack (popped, replaced or switched away).
  A screen that is only covered gets neither. The reader saves its progress in
  `on_hide`.

## Command naming rule

Every action id has exactly one owner.

- A surface id is `<surface>:<action>` or `<surface>:<action>:<arg>`. The surface
  is the folder name: `home`, `library`, `reader`, `notes`, `settings`. Example:
  `library:book:42`. Each surface parses its ids in its own `commands.rs`.
- The app owns the global ids: `nav:home`, `nav:library`, `nav:notes`,
  `nav:settings`, `open_book:<id or path>`, `back`, `quit`. Global ids never get a
  surface prefix.
- A widget that needs an id from another surface has to ask the owner to add a
  command. Do not parse another surface's id.
- An id with no owner is recorded as unknown, not ignored.

## Adding a surface

1. Create `src/surfaces/<name>/mod.rs` with a type that implements `Surface`.
   Add `commands.rs` if it has its own ids, and `<name>/` modules for helpers.
2. Add `pub mod <name>;` to `src/surfaces/mod.rs`.
3. Add the route variant to `Route` in `src/surfaces/mod.rs`, if the screen needs one.
4. Add one line to the table in `create` in `src/surfaces/mod.rs`. This maps the
   route to the surface.
5. Add `tests/<name>.rs` with headless tests and a golden PNG. Use the helpers in
   `tests/common/mod.rs`.

A route with no surface of its own returns `None` from `create`. The overlay is
such a route. The reader pushes it with its state.

## File ownership

Each surface worker edits only these paths.

| Worker | Owns |
|---|---|
| Reader | `src/surfaces/reader/`, `tests/reader.rs` |
| Home and Library | `src/surfaces/home/`, `src/surfaces/library/`, `tests/home.rs`, `tests/library.rs` |
| Notes and Settings | `src/surfaces/notes/`, `src/surfaces/settings/`, `tests/notes_settings.rs` |

The shell owns everything else in this crate: `src/app.rs`, `router.rs`,
`command.rs`, `surface.rs`, `services.rs`, `backend.rs`, `refresh.rs`, `scan.rs`,
`bin/kaia.rs`, `src/surfaces/mod.rs` (the `Route` enum and the table),
`src/surfaces/common.rs` (shared builders), `tests/app_shell.rs`,
`tests/common/mod.rs`, and this README. A worker who needs a change there stops
and reports it.

## Tests

```sh
source tools/kaia/env.sh
cd rust && cargo fmt -p kaia-app -- --check
cd rust && cargo clippy -p kaia-app --all-targets -- -D warnings
cd rust && cargo test -p kaia-app
```

Goldens live in `tests/golden/`. A test compares its frame to the golden. Run with
`KAIA_UPDATE_GOLDEN=1` to write new goldens. A failed comparison writes
`*.actual.png` next to the golden. Delete that file once the change is checked.
