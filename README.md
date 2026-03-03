# Après Work

A local-first desktop scheduler for the hours outside work. Describe what you want to
do (duration, priority, deadline, labels, optional weekly or monthly recurrence) and
when you are willing to do it (schedule windows such as "weekdays 18:00–23:00, weekends
8:00–22:00"). The scheduler places the work into free slots as time blocks, pushes those
blocks to a dedicated Google Calendar, and re-plans when reality changes.

Built with Tauri 2 (Rust) and Svelte 5. All data lives in a local SQLite file. There is
no server and no account.

## Status

Version 0. Working today, on Linux:

- Tasks with duration, priority, start date, deadline, labels, Markdown description and
  comments
- Scheduling engine: priority and deadline ordering, chunking with a minimum chunk size,
  a continuous-work cap with breaks, a planning horizon, full and incremental reschedule
- Recurring tasks (weekly and monthly cadence) with per-instance deadlines and expiry
- Fixed (locked) chunks, by drag-and-drop or on creation
- Schedule windows that bound when tasks may be placed, with a default schedule
- Status view for deadline violations and unschedulable tasks
- Google Calendar push sync over OAuth2 (PKCE, loopback redirect); the refresh token is
  kept in the OS keyring
- Profiles: separate data sets under one installation
- Backup and restore as a zip archive
- Loopback REST API for scripts and agents (`scripts/api.sh` wraps it)

Windows is a build target with little testing so far. Android is a later goal. Expect
rough edges; the design documents run ahead of the UI polish.

## Build and run

Prerequisites: Rust 1.85 or newer, Node.js 20 or newer, and the Tauri 2 system packages
for your platform (see https://v2.tauri.app/start/prerequisites/). On Linux the keyring
integration also needs `libdbus-1`.

```sh
npm install
npx tauri dev      # run in development mode
npx tauri build    # production bundle
```

### Google Calendar credentials

Calendar sync is compiled in only when you supply your own OAuth client. Create a
"Desktop app" OAuth client in Google Cloud Console with the Calendar API enabled, then
build with the credentials in the environment:

```sh
APRESWORK_GOOGLE_CLIENT_ID=... APRESWORK_GOOGLE_CLIENT_SECRET=... npx tauri build
```

Without them the app runs with the calendar provider disabled. Keep the credentials out
of version control.

## Data and API

- Data directory on Linux: `~/.local/share/com.apreswork.app/` (SQLite in WAL mode).
  Do not edit the database by hand; use the app or the API.
- REST API: `http://127.0.0.1:19532` (override the port with `APRESWORK_API_PORT`).
  Loopback only and unauthenticated; the server validates the `Host` header as a
  DNS-rebinding defence. `bash scripts/api.sh` with no arguments lists the wrapped
  commands.

## Development

```sh
npm run check                                   # svelte-check + tsc
npm run lint                                    # eslint
npm run test                                    # vitest
cargo test --manifest-path src-tauri/Cargo.toml
bash scripts/coverage.sh                        # Rust coverage against a ratcheted floor
```

`lefthook` installs the pre-commit hooks on `npm install`: rustfmt, clippy, eslint,
prettier, svelte-check, licence headers, a file-length cap, the Tauri invoke contract
check, gitleaks, and the swarm review gate. The gate (`.quality/gate.json`) requires
every changed source file to carry a recorded review in `.quality/ledger.json`. The
reviewing agents are defined outside this repository, so on a fresh clone set `mode`
to anything other than `enforce` to make the gate advisory. `gitleaks` must be
installed, either on `PATH` or at `~/go/bin/gitleaks`.

Engineering rules, architecture invariants and the verification checklist live in
`CLAUDE.md`.

## Documentation

- `REQUIREMENTS.md` — MoSCoW requirements
- `DESIGN.md` — implementation design: modules, domain models, traits, services, schema
- `ARCHITECTURE.md` — layers, state machines, reschedule and sync flows
- `SCHEDULER_ALGORITHM.md` — the scheduling algorithm with complexity analysis
- `docs/quality/swarm-review-gate.md` — design of the per-file review swarm and its
  ledger gate

## Licence

Apache License 2.0. See `LICENSE` and `NOTICE`.
