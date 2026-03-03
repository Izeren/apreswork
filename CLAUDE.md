# Après Work

## What is this?
A lightweight desktop task scheduler for after-work activities with Google Calendar integration and a Claude-friendly API.

## Tech Stack
- **Frontend**: Svelte 5 + TypeScript + Vite
- **Backend**: Tauri 2 (Rust)
- **Database**: SQLite via rusqlite (bundled, no system dependency)
- **Target platforms**: Linux, Windows desktop (Android later)

## Architecture Decisions
- Local-first: all data stored in a local folder (no cloud dependency)
- Backup: manual export/import (zip the data folder)
- Google Calendar integration (OAuth2 + REST API from Rust side)
- SQLite bundled into the binary — no system install needed on any platform
- Monolith with modular internals (traits for DI, layered architecture)

## Project Structure
```
src/                  # Svelte frontend (TypeScript)
src-tauri/            # Tauri/Rust backend
  src/lib.rs          # Rust commands entry point
  Cargo.toml          # rusqlite bundled
vite.config.ts        # build output -> build/
```

## Design Documents
- `REQUIREMENTS.md` — MoSCoW requirements (M/S/C/W)
- `DESIGN.md` — Implementation design (architecture, models, traits, services, schema)
- `ARCHITECTURE.md` — Layer diagram, module map, state machines, reschedule and sync flows
- `SCHEDULER_ALGORITHM.md` — Scheduler pseudo-algorithm with O-notation analysis
- `docs/quality/swarm-review-gate.md` — swarm review gate and ledger design

Task tracking lives in the app itself (`bash scripts/api.sh`), not in a markdown file.

## Dev Commands
- `npx tauri dev` — run the app in dev mode
- `npx tauri build` — build production binary
- `npm run build` — build frontend only
- `npm run check` — svelte-check + typecheck
- `npm run lint` — ESLint (includes the sonarjs bug/code-smell rules; they apply to `.svelte` script blocks too)
- `npm run format:check` — Prettier check
- `npm run format` — Prettier auto-fix
- `npm run dup` — jscpd copy-paste detector over `src/` + `src-tauri/src/` (standing gate, not a one-off: threshold in `.jscpd.json` fails the run when duplication grows past the recorded baseline; read the per-clone listing, not just the exit code)
- `bash scripts/coverage.sh` — Rust coverage verification. Enforces the hard floor, emits branch-gap and changed-code reports, and should be read rather than treated as a single pass/fail number. Use `-o FILE` to write results to a file.
- `python3 scripts/swarm/ledger.py` — the swarm ledger: one script, one subcommand per job (`--help` lists them). **Every in-scope file must be reviewed at exactly the content being committed**, and every finding on it settled — pre-existing ones included. A finding is cleared by fixing it, or, when it is a false positive, by a rejection that a verifier upholds. Ledger: `.quality/ledger.json` — `{path: {blob, findings}}`, machine-written by `ledger.py rule` and `ledger.py record`, never hand-edit, and staged with the commit that earned it. `gate` runs from the pre-commit hook; never invoke it by hand.
- `python3 scripts/swarm/ledger.py plan --all` — every in-scope file still owing a review.

## Development Rules

### Decision Ladder — stop at the first rung that holds

Before writing code, walk this list top-down. Ship the first option that works.

1. **Does this need to exist?** → skip it (YAGNI)
2. **Std / built-in does it?** → `std`, SQL built-in, browser/HTML native, Svelte built-in
3. **Platform / runtime does it?** → Tauri API, SQLite function, CSS feature
4. **Installed dep already does it?** → chrono, serde, rusqlite, vitest, etc.
5. **One line / one expression?** → write that
6. **Only then** → the minimum correct implementation

Not negligent: security, data-loss prevention, accessibility, and trust-boundary validation are never skipped. The ladder targets *accidental complexity*, not *essential requirements*.

### Architecture Invariants (verify before touching scheduler/services/stores/UI data flow)

1. **Layering**: commands (Tauri + REST handlers) stay thin — parse, call service, trigger, return. Services depend on `&dyn Store` only. The scheduler engine gets everything via `ScheduleInput`, never touches storage.
2. **One definition per policy**: task scheduling order, reschedule trigger mode/immediacy, and any cross-module comparator/table must have exactly ONE definition all call sites import. A second copy is a bug even if identical today.
3. **Time comes from a clock provider**: production code that needs *now* takes it from an injected clock. Service, scheduler, and domain functions keep taking `now: DateTime<Utc>` explicitly; the layers above them read a `ClockProvider` rather than the wall clock, and only the composition root constructs the real one. **No test reads the wall clock at all** — tests must execute deterministically, so they inject the deterministic clock instead of calling `Utc::now()` / `new Date()` / `Date.now()`, and there is **exactly one** testing clock implementation per side (Rust, frontend) that every test reuses: a second `test_now()` with its own epoch is a bug even where it works today. A component initializer, a lazy default (`now ?? new Date()`), a `#[cfg(test)]` fixture builder, and a "placeholder, overwritten later" write are all covered — there is no carve-out small enough. (Owner ruling, upheld twice on decision requests.)

   *Current state*: neither clock provider exists yet, so `Utc::now()` is still tolerated in the command/trigger layer (Tauri commands, REST handlers, the reschedule trigger, timer callbacks) and its frontend equivalent in event handlers and timer callbacks. `src/lib/app-clock.ts` (which exports `appClock: () => Date = () => new Date()`) is the authorized frontend composition-root clock binding — the one module that reads the wall clock to feed the `getNow` prop chain. It is tolerated under this carve-out until Clock 5/6 replaces it with a real ClockProvider. Everything else is existing debt, tracked as six backlog tasks `Clock 1/6`–`Clock 6/6`; don't add to it.
4. **Transactions**: per-store-method transactions exist; `Store::with_tx` also exists for cross-method atomicity. Mark any multi-call mutation sequences NOT yet wrapped with the standard TODO; don't claim atomicity in comments or docs.
5. **Status transitions own their side effects**: changing a task's status must handle its chunks per the state machine (ARCHITECTURE.md §4–5) — e.g. leaving `Scheduled` deletes non-fixed, non-completed chunks.
6. **Frontend data flow**: mutations that trigger a backend reschedule ⇒ refetch the visible range (cascades move other chunks). Stores hold cross-view shared state only; don't add a store without a consumer.
7. **Docs move with code**: adding/moving/renaming a module updates DESIGN.md §2 + ARCHITECTURE.md §2 in the same change.

### Commit Conventions
- Conventional Commits: `type(scope): description` (max 72 chars subject)
- Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
- Agent attribution trailer:
  - Claude-authored commits: `Generated By: Claude Opus 4.6` (or whichever model)
  - Codex-authored commits: use Codex's own attribution/signature line when available

### Git Workflow (human review loop)
- The repo owner curates staging/commits himself in his IDE. Re-run `git status` before assuming tree state; never stage, unstage, or commit beyond what's explicitly asked.
- An `lgtm` HEAD commit (e.g. `feat(cadence): lgtm`) is a **mutable review checkpoint**, not finished work — the owner amends reviewed hunks into it and renames it when the batch is done. Never treat it as final, never amend into it, and never diff against `HEAD~1` to find "the change" (use `git status` + `git diff HEAD`).
- Keep separate concerns as separate commits; propose commit splits only after the owner says review is done.

### Task Sizing
- Leaf tasks target ~≤300 LOC main / ~≤1000 LOC total (incl. tests) — the same 1000-line bar as the per-file length limit.
- Tests should not need ~4× the main LoC. Needing more usually means a missing test harness/doubles (schedule that as its own task) or insufficient parametrization — fix that instead of writing more near-duplicate tests.
- Order tasks by dependency so something is manually testable as early as possible.

### File Headers
Every new `.rs`, `.ts`, `.svelte`, `.css` file MUST start with an SPDX header:
- `.rs` / `.ts`: `// Copyright 2026 Aleksandr Iushmanov (@izeren)` + `// SPDX-License-Identifier: Apache-2.0`
- `.svelte`: `<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->` + `<!-- SPDX-License-Identifier: Apache-2.0 -->`
- `.css`: `/* Copyright 2026 Aleksandr Iushmanov (@izeren) */` + `/* SPDX-License-Identifier: Apache-2.0 */`

Run `bash scripts/add-license-headers.sh` to auto-add missing headers.

### Rust
- `cargo fmt --all -- --check` must pass
- `cargo clippy --all-targets --all-features -- -D warnings` must pass
- `cargo deny check` must pass (run from `src-tauri/`)
- Clippy pedantic is enabled — follow its suggestions or `#[allow]` with justification comment
- Never include tokens/credentials in error messages or logs
- SQL injection prevention: all runtime values via `?` bind parameters, never `format!`/string interpolation (bind params are treated as data, not SQL)
- Services: stateless free functions — `fn do_thing(store: &dyn Store, input: ...) -> Result<T, AppError>`
- Commands: thin wrappers — extract State, call service, return Result
- Store trait: `&self` with interior mutability (`Mutex<Connection>`)
- IDs: `uuid::Uuid::now_v7().to_string()` for all new entity IDs
- Labels: stored in join tables, denormalized into `Vec<String>` on read
- Cadence: serialized as `cadence_type` (TEXT) + `cadence_data` (JSON) in DB
- Test utilities: extract shared helpers (e.g. `memory_db()`, `table_exists()`) into `#[cfg(test)]` utility modules when used across test modules

### TypeScript / Svelte
- `npm run lint` must pass
- `npm run format` must pass (use `format` not `format:check` — the colon breaks permission patterns)
- `npm run check` (svelte-check) must pass
- All Tauri commands invoked via typed wrappers in `api.ts` — never raw `invoke()` in components
- Serialization contract: DateTime as ISO 8601 string, `Option<T>` as `T | null`, enums as discriminated unions with `type` field
- Async API calls in `$effect`: always add `.catch()` that resets loading state and shows an error toast
- Error toasts: errors arrive as `{ error, message }`; show `message` for `error === "validation"` (user-actionable), fall back to a generic string otherwise
- State management: class-based stores with `$state` fields (DESIGN.md §9.4)
- Routing: hash-based with `$state`, `parseHash`, `navigate()` — no external router
- CSS: custom properties for theming, component-scoped styles by default
- Components: props via `$props()`, events via callback props (not `createEventDispatcher`)

### Testing
- TDD: write tests before implementation
- Coverage expectation: get as close to 100% as reasonably practical. `90%` is a hard floor for weird or low-value cases, not the target.
- Uncovered lines or branches are acceptable only when the testing cost is disproportionate or the code is not worth deeper exercise. Those cases should be rare and should be explicitly justified.
- Run `bash scripts/coverage.sh -o /tmp/coverage-result.txt` after implementing tasks with tests, then read the output file and the generated coverage artifacts.
- Review the generated `coverage/branch-gaps.txt` and `coverage/changed-code-coverage.txt` reports. Do not stop at the headline percentage.
- Exceptions: pure data structs, serde derives, and Tauri command thin wrappers
- **No module mocking.** `vi.mock` / `vi.doMock` are lint-errors (`no-restricted-syntax`): they replace a whole module for the file, so the test stops exercising the real one and keeps passing when it changes. Inject the collaborator instead — an api object as a prop, or the dependency as a parameter. Injected doubles built with `vi.fn()` are fine and are the point of the DI seams; `vi.mocked()` is a type cast, not a mock. If a module singleton with import-time side effects leaves no seam, make it injectable rather than stubbing it. The two `eslint-disable`s for `router.svelte` are the only exceptions and are on their way out.
- **All Tauri calls go through `src/lib/api.ts`.** Nothing else may import `invoke` from `@tauri-apps/api/core`. Frontend tests inject doubles for the api and Rust tests call the command bodies directly, so the command name and argument keys are covered by no test at all — `scripts/check_invoke_contract.py` checks them statically instead (name is registered, keys match the parameters camelCase → snake_case, required parameters supplied). Adding an api wrapper means adding the Rust command in the same change.

### Verification Checklist (run before declaring a task done)

Run each command as a **separate** Bash call. **DO NOT** chain with `&&`, pipe with `|`, or redirect with `2>&1`.

1. `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`
3. `cargo test --manifest-path src-tauri/Cargo.toml`
4. `bash scripts/coverage.sh -o /tmp/coverage-result.txt` (if task includes tests), then read `/tmp/coverage-result.txt`, `coverage/branch-gaps.txt`, and `coverage/changed-code-coverage.txt`
5. `npm run format`, `npm run lint`, `npm run check` (if frontend changed) — each as a separate Bash call
6. `npm run dup` — duplication gate; on failure, deduplicate the new code (or, with owner sign-off, adjust the `.jscpd.json` threshold)
7. `python3 scripts/check_invoke_contract.py` (if `api.ts` or any Tauri command changed)

### Command Permission Patterns

Commands must match the auto-approved permission patterns. Mismatched commands trigger manual approval prompts.

- **git**: Use `git status`, `git add`, `git commit` etc. directly — **never** `git -C /path ...`
- **cargo**: Use `cargo test --manifest-path src-tauri/Cargo.toml` — matches `Bash(cargo test:*)`
- **Never chain** with `&&`, `;` — run each command as a separate Bash call. Never append `&& echo Exit`, `; echo Exit`, or similar suffixes.
- **Never pipe or redirect** — no `| head`, `| tail`, `| grep`, `2>&1`, `echo ... |`
- **scripts**: Always use relative paths — `bash scripts/coverage.sh`, `bash scripts/add-license-headers.sh` — **never** absolute paths like `bash /home/.../scripts/...`
- **API calls**: Use `bash scripts/api.sh <command> …` — matches `Bash(bash scripts/api.sh:*)`. Running it with no arguments prints the command list. **Never** use raw `curl` for API calls.
- **No `node -e`** — use the Read tool to inspect `node_modules` files instead. `node -e` triggers manual approval prompts.
- **No heredoc** — never `git commit -m "$(cat <<'EOF' … EOF)"`; heredoc cannot be auto-approved. Use one or more plain `-m "…"` flags.
- **Files**: find/read/search files with the Glob/Read/Grep tools — not `cat`/`head`/`tail`/`wc`/bash `grep`/`find`/`rg`. Bash is for real shell operations (git, cargo, npm, scripts).
- **Spawning agents**: When spawning built-in agents (Explore, Plan, general-purpose), include the Command Permission Patterns section in the prompt — built-in agents do not read CLAUDE.md or agent .md files automatically. Specifically instruct them to use Grep/Glob tools instead of bash grep/find/rg.

### Live App Data

- **Never edit the live SQLite DB directly** (`<data dir>/profiles/<profile id>/apreswork.db`, WAL; one DB per profile) — not even for one-off fixes. All task/schedule manipulation goes through `bash scripts/api.sh` (REST).
- The REST server is embedded in the running Tauri app: new or changed endpoints exist at runtime only after a rebuild + app restart.
- `profile-switch` is **shared global state** — it moves every session and the running
  app, so switch, do the work, and switch straight back with the `EXPECTED_PROFILE_ID`
  guard on both hops. Profiles do not share schedules: a `schedule_id` from one profile
  404s on another, so null it when copying a task across.

### Google APIs (hard rule)

- Agents (main session AND subagents) must **never** interact with Google Calendar or any other Google API directly — no direct network calls, no OAuth flows, no MCP Google tools. Never trigger OAuth automatically: the owner authorizes manually and drives ALL live Google testing. Calendar behavior is exercised through unit tests with injected providers.

## Agent Instructions

Delegated agents (dev, reviewer, swarm finder, verifier) carry their own instruction files outside this repo; a planner assigns the grade per task. Agents load this CLAUDE.md for project-specific conventions, architecture invariants, and verification commands. Reviewer agents are expected to use coverage artifacts when they exist. A passing threshold alone is not enough: they should inspect uncovered changed lines/branches and flag gaps that are not clearly justified.

### Rule precedence

**An agent's own instructions outrank this file.** Agents carry rules that are deliberately narrower than the project's general ones, so a finding this document does not mention is not thereby wrong — absence from CLAUDE.md is not a refutation. Swarm findings name their source: `RULE(agent): "…"` for the agent's own rule, `RULE(<file> §<section>): "…"` for a project rule.

A verifier that believes a `RULE(agent)` finding genuinely conflicts with a rule here still rules on the source as it finds it — there is no verdict for parking the question — and proposes the rule the owner should codify. On an upheld rejection the proposal is stored beside it in the ledger, under `rule`, where every future finder on that file reads it; on a denial it comes back in the verifier's result line for the session to relay. Verifiers do not invent carve-outs to settle the conflict themselves — an unwritten exception applied inconsistently across runs is worse than an open question.

A ruling that keeps the rule is written into this file or into the agent's own instructions, so the next sweep verifies it normally instead of re-proposing the same thing.
