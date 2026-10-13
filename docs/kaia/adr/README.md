# Architecture decision records

These records explain the first architecture decisions for Kaia Reader. They come from the first architecture records list in `docs/kaia/INITIAL_BACKLOG.md`.

Every record starts as proposed. Accept a record only after review and prototype evidence. Amend a record when prototype evidence changes its assumptions. Each record lists the evidence needed to accept it.

| ADR | Title | Status | Date |
|---|---|---|---|
| [ADR-001](0001-incremental-migration.md) | Incremental application migration | Superseded by ADR-008 | 2026-10-13 |
| [ADR-002](0002-one-coordinator-and-luajit-runtime.md) | One coordinator, one LuaJIT runtime and an initial Rust C ABI | Proposed | 2026-10-13 |
| [ADR-003](0003-legacy-plugin-compatibility-profile.md) | Explicit, versioned legacy plugin compatibility profile | Proposed | 2026-10-13 |
| [ADR-004](0004-retained-engines-and-kobo-integration.md) | Existing document engines and Kobo integration retained | Proposed | 2026-10-13 |
| [ADR-005](0005-single-data-owner.md) | Single data owner, isolated profile, reversible migrations and durable exports | Proposed | 2026-10-13 |
| [ADR-006](0006-software-rendered-eink-ui.md) | Software-rendered E Ink UI through one refresh path | Proposed | 2026-10-13 |
| [ADR-007](0007-product-identity-trusted-plugins-recovery.md) | Separate product identity, trusted legacy plugins and external recovery | Proposed | 2026-10-13 |
| [ADR-008](0008-rust-rewrite.md) | Rewrite the app in Rust, add Lua plugin compatibility afterwards | Accepted | 2026-10-13 |
