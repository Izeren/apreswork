# Swarm review gate

Per-file agentic code review with a committed ledger and a pre-commit gate. This
document describes the mechanism as implemented by `scripts/swarm/ledger.py`,
`.quality/gate.json`, `.quality/ledger.json` and the `swarm-gate` hook in
`lefthook.yml`. The agents that do the reviewing and the `/swarm-review` skill that
drives them are defined outside this repository, in the owner's agent rules repo. A
clone without them can still run the gate; set `mode` in `.quality/gate.json` to
anything other than `enforce` to make it advisory.

## 1. Purpose

Every in-scope source file (`src/**` TypeScript and Svelte, `src-tauri/src/**` Rust,
production and test alike) must be reviewed at exactly the content being committed, and
every finding raised on it must be settled. The contract is deliberately binary: **a
reviewed file has zero open findings.** A finding is fixed, or it is a false positive
that somebody other than its author agreed to. There is no third outcome — no "real but
out of scope", no "tracked as debt", no severity that is allowed to stand — so nothing
needs scoring, ratcheting or baselining.

The gate is a guardrail that reminds models of the rules, not a security boundary. Any
agent could forge a ledger entry; no design effort is spent preventing that.

## 2. Roles

| role | who | tier | in flight | does |
|---|---|---|---|---|
| finder | `code-smell-nitpicker`, `doc-slop-reviewer`, `test-param-enforcer` | graduate | 30 | reviews one file, logs its own findings |
| session | the implementing session | — | — | fixes or rejects every finding |
| verifier | `middle-swarm-verifier`; `senior-swarm-verifier` when escalated | middle / senior | 3 / 1 | rules on the session's rejections |
| judge | `judge` | middle-2 | 1 | hears one appeal per round |

Finders are read-only on reviewed code. Each first runs `ledger.py known` for the upheld
rejections on its file, then writes its findings as JSON and hands them to
`ledger.py log`; its chat reply is the script's one output line. Findings never travel
through chat: the session reads payloads only to fix what they describe, and never edits
one.

## 3. State model

| status | written by | lives in | meaning |
|---|---|---|---|
| `unverified` | `ledger.py log` | `.swarm/*.unverified` | a finder raised it; nobody has ruled |
| `fixed` | the session | payload | the session changed the code |
| `rejected` | the session | payload | the session's claim that it is a false positive; unruled |
| `rejected` | a verifier, or the judge on appeal | **the ledger** | the claim was upheld; permanent |

- A `fixed` finding is never verified by an agent. The next finder run is the
  verification: a fix that did not land comes back as a fresh finding at the new blob.
- A denied rejection reverts to `unverified` and keeps both arguments in `prior[]`, so
  an appeal puts the disagreement in front of the judge intact.
- A session can never turn its own argument into a permanent record: `resolve` has no
  path to the ledger, and `rule` cannot mark anything fixed.

Rejecting is deliberately the expensive path. A fix answers a finding by itself; a
rejection retires one with no line of code changing — the one move that can be wrong and
still look like progress — so it is ruled on before the commit that records it lands. In
the first repo-wide sweep under a deferred model, a fifth of the standing rejections
conceded the finding was real and rejected it on scope, which is why "out of scope" and
"tracked as debt" are not rejections at all.

## 4. Findings

```json
{"id": "6f8d2c11-8a4e-4e1b-9a77-2f0b5c3d9e10", "symbol": "resize_chunk",
 "severity": "MAJOR", "count": 1, "summary": "RULE(agent): \"…\" — …",
 "fix": "route the write through sync_task_pinned", "status": "unverified"}
```

- `id` is a UUID4 minted by `log`. It lets one agent name a finding that another reads
  for itself (`ledger.py show --id`) instead of restating it.
- `symbol` is the anchor: the innermost named item containing the finding, or the
  literal `<file>` for a file-scope finding (markup and `<style>` blocks in `.svelte`
  files sit inside no named item). Line numbers are not recorded: they drift under
  concurrent edits, and a symbol locates a finding closely enough in files kept under
  the project's length caps.
- `severity` is `MAJOR` or `MINOR`; anything else normalises to `MINOR`. For
  `test-param-enforcer`, a finding naming `tests [a, b, c]` is re-graded by count: three
  or more merged tests is `MAJOR`, two is `MINOR`. Severity ranks findings for whoever
  fixes them; it never decides whether one counts.
- `count` says at how many sites one rule is violated; informational.
- `summary` names its source: `RULE(agent): "…"` for the finder's own rule,
  `RULE(<file> §<section>): "…"` for a project rule.

Payloads live in gitignored `.swarm/` as
`<agent>.<path with '/' replaced by '__'>.<unverified|verified>`; `.verified` means only
"this finder found nothing at blob X".

## 5. Ledger — `.quality/ledger.json`

The whole durable record, tracked, and staged with every commit that changes it. Two
keys per file:

```json
{"src/lib/utils.ts": {
   "blob": "16f99efe8174025691724e67fb8d7b0d17579edd",
   "findings": {
     "6f8d2c11-8a4e-4e1b-9a77-2f0b5c3d9e10": {
       "agent": "doc-slop-reviewer", "symbol": "formatSpan", "severity": "MINOR",
       "summary": "…", "session_reason": "…", "verifier_reason": "…",
       "verifier": "middle-swarm-verifier", "at": "2026-08-06T09:31:02+00:00",
       "blob": "16f99efe8174025691724e67fb8d7b0d17579edd",
       "rule": "optional: a rule the owner should codify"}}}}
```

- `blob` is the git blob every finder in the file's set agreed it reviewed, and it is
  the entire freshness mechanism. Any difference from the index blob is an unreviewed
  file; there is no tolerance band and no timestamp.
- `findings` holds upheld rejections only, keyed by id, and is `{}` for a clean file.
  A rejection carries the blob the finder actually read; `known` marks it `STALE` once
  the file has changed, so a finder re-judges instead of trusting a ruling made against
  code that no longer exists.
- Two subcommands write the file and they write disjoint keys — `rule --uphold` sets
  `findings`, `record` sets `blob` — under an exclusive `flock`, because verifiers run
  concurrently.

**Why a blob, not a commit.** An entry saying "reviewed at commit C" sits inside C's
tree, so writing C into it changes C; there is no fixed point. A blob is a fingerprint
of one file's bytes with no path or time in it, so the comparison the gate needs is the
comparison the entry makes directly. Two edges: a rename reads as a never-reviewed file
(correct — its context changed), and `git hash-object` must be given a path, not stdin,
so that `.gitattributes` filters produce the blob the index holds.

**Why committed, not local.** A consumed local "bill of health" leaves no record, so a
commit that skipped the hook is undetectable afterwards. A committed record survives a
clone and lets the gate ask its question repo-wide.

## 6. Configuration — `.quality/gate.json`

```json
{"mode": "enforce",
 "scope": ["src/**/*.ts", "src/**/*.svelte", "src-tauri/src/**/*.rs",
           "**/*.test.ts", "src-tauri/**/tests/**/*.rs"],
 "finders": {
   "test":       {"match": ["**/*.test.ts", "src-tauri/**/tests/**/*.rs"],
                  "agents": ["code-smell-nitpicker", "doc-slop-reviewer",
                             "test-param-enforcer"]},
   "production": {"match": ["src/**", "src-tauri/src/**"],
                  "agents": ["code-smell-nitpicker", "doc-slop-reviewer"]}}}
```

The first matching finder class wins, so the test class is declared first. The finder
set lives here because `record` cannot know a review is complete without it: a review is
complete only when every configured finder has reported at the same blob.

## 7. Script surface — `scripts/swarm/ledger.py`

| subcommand | who runs it | what it does |
|---|---|---|
| `log <file> --agent --findings` | finder | mints ids, normalises, writes the payload, consumes its input |
| `known --file [--agent]` | finder | upheld rejections on the file, with `STALE` marking |
| `plan [--all]` | session | `<file>\t<finder set>` for every file owing a review; `--all` lists every in-scope file |
| `resolve <payload> --fixed --reject [--escalate]` | session | marks every finding; refuses a partial set |
| `batches` | session | `<tier>\t<payload paths>`, at most 10 source files each |
| `rule <payload> --verifier --uphold --deny [--rule]` | verifier, judge | the only writer of `findings` |
| `show --id` | session | resolves an id against payloads, then the ledger |
| `record` | session | the only writer of `blob` |
| `gate` | pre-commit hook | never run by hand |

`record` refuses a file on three grounds, each of which has been a real bug: a payload
still awaiting action (a finding nobody acted on is not a clean review); a configured
finder that did not report; payloads that disagree on the blob (a session fixed one
finder's finding and re-ran only that finder). Tests: `python3 scripts/swarm/test_swarm.py`.

## 8. Flow

```
1. resolve scope        ledger.py plan
2. finders              ≤30 graduate, the whole batch before anything else
                        → .unverified (findings) or .verified (clean)
3. session              ledger.py resolve — every finding fixed or rejected
   ├─ no rejections ──────────────────────────────────► step 6
   └─ rejections
        4. verify       ledger.py batches → middle (≤3) / senior (1) verifiers
                        upheld → ledger, removed from the payload
                        denied → back to unverified, returned by id
        5. denials      fix, or ONE judge appeal for the whole round
6. re-run finders       the whole finder set of every file touched in 3 or 5
7. record               ledger.py record, then stage .quality/ledger.json
8. report               fixed / rejected / upheld / denied counts to the owner
```

A payload whose findings are all `fixed` is deleted. Step 6 is what produces the
`.verified` payloads step 7 needs: the session's edits keyed the old ones to content that
no longer exists, and mixing a re-run finder with a stale one is exactly what `record`
refuses.

Batching is per tier, not per language, which is why the verifiers are language-agnostic
agents. All of a file's payloads go to the same batch (two agents must never write the
same payload), and a file with any `--escalate` rejection goes whole to the senior batch.
Escalation is the session's own call, for arguments it expects to lose.

## 9. Rule proposals

A verifier rules on the source as it finds it, then says what the owner should codify.
When the finding is accurate but rests on an unsettled rule it **denies**, and states the
proposed rule in its result line. When the finder is the one out of line it **upholds**,
and attaches the rule with `rule --rule "<id>=<rule>"`; the proposal is stored on the
rejection record, where every future finder on that file reads it. A rule cannot ride on
a denial — there is nothing durable to attach it to. A ruling that keeps the rule is
written into `CLAUDE.md` or the agent's own instructions, so the next sweep verifies it
instead of re-proposing it.

## 10. Commit gate — `ledger.py gate`

Run from the lefthook `pre-commit` section. Three questions, any of which blocks at
`mode: enforce` and only prints otherwise:

1. **Every in-scope tracked file is reviewed at the content being committed.** The
   ledger is read out of the index (`git show :.quality/ledger.json`), never off disk,
   and the sweep is repo-wide: a file that changed and then stopped being touched is
   exactly the one a staged-only check never asks about again.
2. **No `.unverified` payload anywhere in `.swarm/`.** An outstanding payload is a round
   somebody started and abandoned.
3. **If `.quality/ledger.json` changed, it is staged.** Check 1 reads the index, so this
   names the actual cause when a recorded review still reports as unreviewed.

The hook never invokes models and reads no file content; the whole scope costs two
`git ls-files` calls and a dictionary lookup per file.

## 11. Decisions

| decision | choice | rejected |
|---|---|---|
| verdict store | hash-keyed sidecar ledger, tracked | in-file annotations (source churn); git notes (fragile tooling); a local consumed bill (no record after the fact) |
| freshness | binary blob match | a drift band by diff size (buys the right to accumulate unreviewed lines and needs a threshold to defend); commit hashes (no fixed point); timestamps |
| gate coverage | every in-scope file, every commit | staged files only (a file changed once and then left alone is never asked again) |
| open findings | none survive: fixed, or rejected and upheld | scores and ratchets ("no worse" is satisfied by standing still); "confirmed but left" and "downgraded" verdicts (debt with no owner) |
| rejections at commit | ruled on before the commit lands | parked and adjudicated on the owner's schedule (nothing read them until the pile was large) |
| fixes | never verified by an agent | verifying every finding (pays a mid-tier agent to agree about findings nobody contested) |
| anchor | symbol, or `<file>` | line numbers (drift under concurrent edits) |
| finding transport | `.swarm/` payloads written by the agents' own script calls | chat envelopes parsed by the session (most of a run's cost was the session re-reading findings) |
| suppressions | none; an upheld rejection is the permanent silence | owner-written substring matches the finders never see |
| script surface | one script, one subcommand per job; `resolve` and `rule` split | a script per stage sharing a library |

## 12. Non-goals

- No adversarial hardening (§1).
- No dependency blobs: an entry covers one file and says nothing about its imports
  changing. The finder contract bounds reads to the target plus a direct import.
- No commit hashes anywhere, and no timestamp beside a fingerprint.
- No scores, weights or aggregate metrics; the report is the counts in step 8.
- The ledger is written `sort_keys=True`, in path order, so an entry stays at a stable
  place in the diff instead of moving with its finding count.
