#!/usr/bin/env python3
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0
"""The swarm ledger: query it, write to it, and gate commits on it.

`.quality/ledger.json` is `{path: {"blob": str, "findings": {id: finding}}}`. `blob`
is the content every finder agreed it reviewed; freshness is that blob compared with
the one being committed. `findings` holds upheld rejections only, keyed by id, and is
empty for a file that came back clean. In-flight rounds live in gitignored `.swarm/`
payloads, so the committed ledger is the whole durable record.
"""
from __future__ import annotations

import argparse
import fcntl
import json
import re
import subprocess
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

QUALITY_DIR, SWARM_DIR = ".quality", ".swarm"
LEDGER_FILE, CONFIG_FILE = "ledger.json", "gate.json"
SEVERITIES = ("MAJOR", "MINOR")
FILE_SCOPE = "<file>"
GROUP_RULE_AGENT = "test-param-enforcer"
GROUP_TESTS_RE = re.compile(r"tests \[([^\]]+)\]")
PAIR_RE = re.compile(r"^([0-9a-fA-F-]+)\s*=\s*(.+)$", re.DOTALL)
BATCH_FILES = 10


# ------------------------------------------------------------------ plumbing

def git(root: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=root, capture_output=True,
                          text=True, check=True).stdout

def repo_root(anchor: str | None = None) -> Path:
    """The repo holding `anchor`, or the one the caller is standing in.

    Dispatched finders inherit the parent session's cwd, which is not necessarily the
    repo under review, so any subcommand handed a path resolves the root from that path
    instead. Deriving it from cwd alone once wrote a payload into a neighbouring repo.
    """
    here = Path.cwd() if anchor is None else Path(anchor).resolve().parent
    return Path(git(here, "rev-parse", "--show-toplevel").strip()).resolve()

def relative(root: Path, value: str) -> str:
    """Repo-relative POSIX path, or a loud failure.

    Never fall back to the value verbatim: an absolute path that survives into
    `payload_path` names a file the repo does not have, and the finder-set check then
    reads that as one more finder that never reported.
    """
    try:
        return Path(value).resolve().relative_to(root).as_posix()
    except ValueError:
        raise ValueError(f"{value} is not inside {root}") from None

def payload_path(root: Path, agent: str, rel: str, state: str) -> Path:
    return root / SWARM_DIR / f"{agent}.{rel.replace('/', '__')}.{state}"

def load_config(root: Path) -> dict:
    return json.loads((root / QUALITY_DIR / CONFIG_FILE).read_text(encoding="utf-8"))

def load_ledger(root: Path) -> dict:
    path = root / QUALITY_DIR / LEDGER_FILE
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}

def update_ledger(root: Path, mutate) -> dict:
    """Read -> mutate -> write under an exclusive lock.

    Three verifiers run at once and every uphold rewrites the whole file, so the read
    is locked too: two that both read before either writes would each save a ledger
    missing the other's ruling.
    """
    path = root / QUALITY_DIR / LEDGER_FILE
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a+", encoding="utf-8") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        handle.seek(0)
        text = handle.read()
        led = json.loads(text) if text.strip() else {}
        mutate(led)
        handle.seek(0)
        handle.truncate()
        handle.write(json.dumps(led, indent=2, sort_keys=True) + "\n")
        return led

def rejections_for(ledger: dict, rel: str) -> list[dict]:
    """Upheld rejections on one file, each carrying its id folded back in."""
    found = ledger.get(rel, {}).get("findings", {})
    return [{"id": fid, **f} for fid, f in sorted(found.items())]

def in_scope(rel: str, scope: list[str]) -> bool:
    return any(PurePosixPath(rel).full_match(p) for p in scope)

def finders_for(rel: str, config: dict) -> list[str]:
    """The finder set this file owes. First match wins, so declaration order matters:
    a test file matches the production globs too."""
    for spec in config.get("finders", {}).values():
        if in_scope(rel, spec.get("match", [])):
            return list(spec.get("agents", []))
    return []

def tracked_in_scope(root: Path, scope: list[str]) -> list[str]:
    return [f for f in git(root, "ls-files").splitlines() if in_scope(f, scope)]

def write_blob(root: Path, rel: str) -> str:
    return git(root, "hash-object", "-w", "--", rel).strip()

def indexed_blobs(root: Path) -> dict[str, str]:
    """Tracked path -> index blob, in one call. NUL-separated so paths needing quotes
    survive; a C-quoted key matches no ledger entry and would read as never reviewed."""
    blobs = {}
    for record in git(root, "ls-files", "-s", "-z").split("\0"):
        meta, _, path = record.partition("\t")
        fields = meta.split()
        if path and len(fields) > 1:
            blobs[path] = fields[1]
    return blobs

def worktree_blobs(root: Path, paths: list[str]) -> dict[str, str]:
    """Path -> hash of the content on disk: what a finder would read, as opposed to
    what a commit would contain."""
    present = [p for p in paths if (root / p).is_file()]
    if not present:
        return {}
    return dict(zip(present, git(root, "hash-object", "--", *present).split()))

def owes_review(entry: dict | None, blob: str | None) -> bool:
    """Any difference counts — there is no tolerance band. `blob is None` means no
    content to compare, which is not the same as unreviewed."""
    if entry is None or "blob" not in entry:
        return True
    return blob is not None and blob != entry["blob"]

def read_payloads(root: Path, suffixes=(".verified", ".unverified")) -> list[tuple]:
    swarm = root / SWARM_DIR
    out = []
    for path in sorted(swarm.iterdir()) if swarm.is_dir() else []:
        if path.suffix in suffixes:
            try:
                out.append((path, json.loads(path.read_text(encoding="utf-8"))))
            except json.JSONDecodeError:
                continue
    return out

def open_payload(value: str) -> tuple[Path, dict]:
    path = Path(value)
    if path.suffix != ".unverified" or not path.is_file():
        raise ValueError(f"not an existing .unverified payload: {value}")
    return path, json.loads(path.read_text(encoding="utf-8"))

def parse_pairs(specs: list[str], flag: str) -> dict[str, str]:
    """`--<flag> "<id>=<reason>"`, repeatable. The reason is required: upheld
    rejections are the dataset the finders are tuned from, so "not real" is a
    wasted sample."""
    pairs: dict[str, str] = {}
    for spec in specs:
        match = PAIR_RE.match(spec.strip())
        if not match or not match.group(2).strip():
            raise ValueError(f"bad --{flag} '{spec}': expected '<finding id>=<reason>'")
        if match.group(1) in pairs:
            raise ValueError(f"duplicate --{flag} for {match.group(1)}")
        pairs[match.group(1)] = match.group(2).strip()
    return pairs

def save(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


# ------------------------------------------------------------------- writers

def normalize(raw: dict, agent: str) -> dict:
    summary = str(raw.get("summary", "")).strip()
    if not summary:
        raise ValueError("missing 'summary'")
    severity = str(raw.get("severity", "")).strip().upper()
    if severity not in SEVERITIES:
        severity = "MINOR"
    if agent == GROUP_RULE_AGENT and (group := GROUP_TESTS_RE.search(summary)):
        names = [n for n in group.group(1).split(",") if n.strip()]
        severity = "MAJOR" if len(names) >= 3 else "MINOR"
    count = re.search(r"\d+", str(raw.get("count", "1")))
    # `<file>` is a first-class anchor, not a degraded one: a finder that cannot name
    # the enclosing item should say so rather than invent one. Lines are not recorded
    # at all — they drift under concurrent edits.
    return {"id": str(uuid.uuid4()),
            "symbol": str(raw.get("symbol", "")).strip() or FILE_SCOPE,
            "severity": severity,
            "count": max(1, int(count.group())) if count else 1,
            "summary": summary,
            "fix": str(raw.get("fix", "")).strip(),
            "status": "unverified"}

def log(root: Path, rel: str, agent: str, payload: dict, now: str) -> str:
    raw = payload.get("findings")
    if not isinstance(raw, list):
        raise ValueError("payload must contain a 'findings' list")
    findings = []
    for i, item in enumerate(raw):
        if not isinstance(item, dict):
            raise ValueError(f"finding {i}: not an object")
        try:
            findings.append(normalize(item, agent))
        except ValueError as exc:
            raise ValueError(f"finding {i}: {exc}") from exc
    # Any finding at all routes to verification, whatever its severity: an unjudged
    # MINOR is indistinguishable from a false positive.
    state = "unverified" if findings else "verified"
    path = payload_path(root, agent, rel, state)
    save(path, {"file": rel, "agent": agent, "blob": write_blob(root, rel),
                "reviewed_at": now, "findings": findings})
    payload_path(root, agent, rel,
                 "verified" if findings else "unverified").unlink(missing_ok=True)
    majors = sum(1 for f in findings if f["severity"] == "MAJOR")
    return (f"logged: {agent} on {rel} — {len(findings)} finding(s), {majors} MAJOR"
            f" -> {SWARM_DIR}/{path.name}")

def resolve(path: Path, payload: dict, fixed: set[str], rejected: dict[str, str],
            escalated: set[str]) -> str:
    ids = {str(f["id"]) for f in payload["findings"]}
    if unknown := sorted((fixed | set(rejected) | escalated) - ids):
        raise ValueError(f"no such finding in this payload: {', '.join(unknown)}")
    if both := sorted(fixed & set(rejected)):
        raise ValueError(f"marked both fixed and rejected: {', '.join(both)}")
    if stray := sorted(escalated - set(rejected)):
        raise ValueError(f"--escalate only applies to a rejection: {', '.join(stray)}")
    if missing := sorted(ids - fixed - set(rejected)):
        raise ValueError("every finding needs a mark, whatever its severity; missing "
                         + ", ".join(missing))
    for item in payload["findings"]:
        fid = str(item["id"])
        item.pop("reason", None)
        item.pop("escalate", None)
        item["status"] = "fixed" if fid in fixed else "rejected"
        if fid in rejected:
            item["reason"] = rejected[fid]
            if fid in escalated:
                item["escalate"] = True
    head = f"resolved: {payload['agent']} on {payload['file']}"
    if not rejected:
        path.unlink()
        return f"{head} — {len(fixed)} all fixed, payload removed"
    save(path, payload)
    return f"{head} — {len(fixed)} fixed, {len(rejected)} rejected, awaiting a verifier"

def rule(root: Path, path: Path, payload: dict, upheld: dict[str, str],
         denied: dict[str, str], rules: dict[str, str], verifier: str,
         now: str) -> str:
    by_id = {str(f["id"]): f for f in payload["findings"]}
    ruled = set(upheld) | set(denied)
    if orphan := sorted(set(rules) - set(upheld)):
        raise ValueError("a rule proposal rides on an upheld rejection, which is the"
                         f" only ruling with a durable record: {', '.join(orphan)}")
    if unknown := sorted(ruled - set(by_id)):
        raise ValueError(f"no such finding in this payload: {', '.join(unknown)}")
    if both := sorted(set(upheld) & set(denied)):
        raise ValueError(f"both upheld and denied: {', '.join(both)}")
    if stray := sorted(f for f in ruled if by_id[f].get("status") != "rejected"):
        raise ValueError(f"not marked rejected, not yours to rule on: {', '.join(stray)}")
    open_ones = {f for f, v in by_id.items() if v.get("status") == "rejected"}
    if missing := sorted(open_ones - ruled):
        raise ValueError(f"every rejection needs a ruling; missing {', '.join(missing)}")
    # Keyed by id and the value does not repeat it — the ledger stores it once, as the
    # key, and `rejections_for` folds it back in on read. `rule` is optional and rides
    # here rather than in a spool of its own: the proposal is only ever read alongside
    # the rejection that prompted it.
    records = {fid: {"agent": payload["agent"], "symbol": by_id[fid]["symbol"],
                     "severity": by_id[fid]["severity"],
                     "summary": by_id[fid]["summary"],
                     "session_reason": by_id[fid].get("reason", ""),
                     "verifier_reason": reason, "verifier": verifier,
                     "at": now, "blob": payload["blob"],
                     **({"rule": rules[fid]} if fid in rules else {})}
               for fid, reason in upheld.items()}
    for fid, reason in denied.items():
        item = by_id[fid]
        # Both sides survive: a disagreement with one side overwritten is not one.
        item.setdefault("prior", []).append(
            {"status": "rejected", "reason": item.get("reason", "")})
        item["prior"].append({"status": "denied", "reason": reason, "by": verifier})
        item["status"] = "unverified"
        item.pop("reason", None)
        item.pop("escalate", None)
    payload["findings"] = [f for f in payload["findings"] if str(f["id"]) not in upheld]
    if records:
        # Sets `findings` only, never `blob`: the review is not complete until the
        # whole finder set comes back clean, which is `record`'s question.
        update_ledger(root, lambda led: led.setdefault(payload["file"], {})
                      .setdefault("findings", {}).update(records))
    head = (f"ruled: {payload['agent']} on {payload['file']} — {len(upheld)} upheld,"
            f" {len(denied)} denied")
    if all(f.get("status") == "fixed" for f in payload["findings"]):
        path.unlink()
        return head + ", payload removed"
    save(path, payload)
    return head + f", {len(denied)} back to the session"

def record(root: Path) -> str:
    """Three refusals, each of which has been a real bug: a finding nobody acted on is
    not a clean review; "no open payload" is a different question from "every finder
    ran"; and payloads disagreeing on the blob describe different content."""
    config = load_config(root)
    verified: dict[str, dict[str, tuple]] = {}
    pending: dict[str, list[str]] = {}
    for path, payload in read_payloads(root):
        if path.suffix == ".verified":
            verified.setdefault(payload["file"], {})[payload["agent"]] = (path, payload)
        else:
            pending.setdefault(payload["file"], []).append(path.name)
    accepted, consumed, lines = {}, [], []
    for rel in sorted(verified):
        got = verified[rel]
        missing = [a for a in finders_for(rel, config) if a not in got]
        blobs = {p["blob"] for _, p in got.values()}
        if not (root / rel).is_file():
            lines.append(f"  skipped {rel}: no longer on disk")
        elif rel in pending:
            lines.append(f"  refused {rel}: {len(pending[rel])} payload(s) awaiting"
                         f" action — {', '.join(sorted(pending[rel]))}")
        elif missing:
            lines.append(f"  refused {rel}: missing {', '.join(missing)}")
        elif len(blobs) > 1:
            lines.append(f"  refused {rel}: finders disagree on the blob"
                         f" ({', '.join(sorted(b[:8] for b in blobs))}) — re-run the"
                         " whole set")
        else:
            accepted[rel] = blobs.pop()
            consumed.extend(p for p, _ in got.values())
            lines.append(f"  recorded {rel} ({len(got)} finder(s))")

    def apply(led: dict) -> None:
        for target, blob in accepted.items():
            led.setdefault(target, {})["blob"] = blob
            led[target].setdefault("findings", {})

    if accepted:
        update_ledger(root, apply)
        for path in consumed:
            path.unlink(missing_ok=True)
    return "\n".join([f"recorded {len(accepted)} file(s)", *lines]) + "\n"


# ------------------------------------------------------------------- queries

def known(root: Path, rel: str, agent: str | None) -> str:
    rows = [r for r in rejections_for(load_ledger(root), rel)
            if not agent or r.get("agent") == agent]
    if not rows:
        return f"no upheld rejections on record for {rel}\n"
    current = write_blob(root, rel) if (root / rel).is_file() else None
    lines = [f"upheld rejections on {rel} — do not raise these again unless the code"
             " has materially changed:", ""]
    for row in rows:
        # A ruling argued against content that has since changed does not carry: the
        # code it was about may be gone, so the finder judges it fresh.
        stale = "" if not current or row.get("blob") == current else \
            " STALE (file changed since this ruling — re-judge)"
        lines += [f"[{row['id']}] {row.get('agent', '?')} — "
                  f"{row.get('symbol', '?')} ({row.get('severity', '?')}){stale}",
                  f"    claim:    {row.get('summary', '')}",
                  f"    session:  {row.get('session_reason', '')}",
                  f"    upheld:   {row.get('verifier_reason', '')}"]
        if row.get("rule"):
            lines.append(f"    proposed: {row['rule']}")
        lines.append("")
    return "\n".join(lines)

def show(root: Path, fid: str) -> tuple[str, dict] | None:
    """Ids exist so one agent can name a finding and another read it, instead of the
    first restating the whole thing in a result the main session pays to read."""
    for path, payload in read_payloads(root):
        for item in payload.get("findings", []):
            if str(item.get("id")) == fid:
                return f"{path.name} ({payload['file']})", item
    ledger = load_ledger(root)
    for rel in sorted(ledger):
        if (hit := ledger[rel].get("findings", {}).get(fid)) is not None:
            return f"ledger ({rel})", {"id": fid, **hit}
    return None

def plan(root: Path, every: bool) -> str:
    config = load_config(root)
    scoped = tracked_in_scope(root, config["scope"])
    ledger, blobs = load_ledger(root), worktree_blobs(root, scoped)
    open_rounds = {p["file"] for _, p in read_payloads(root)}
    return "".join(f"{rel}\t{','.join(finders_for(rel, config))}\n" for rel in scoped
                   if every or rel in open_rounds
                   or owes_review(ledger.get(rel), blobs.get(rel)))

def batches(root: Path) -> str:
    by_file: dict[str, list[Path]] = {}
    tier: dict[str, str] = {}
    for path, payload in read_payloads(root, (".unverified",)):
        rel = payload["file"]
        by_file.setdefault(rel, []).append(path)
        tier[rel] = "senior" if any(f.get("escalate")
                                    for f in payload.get("findings", [])) \
            else tier.get(rel, "middle")
    out = []
    for name in ("middle", "senior"):
        files = sorted(rel for rel, t in tier.items() if t == name)
        for i in range(0, len(files), BATCH_FILES):
            out.append(f"{name}\t" + " ".join(
                str(p) for rel in files[i:i + BATCH_FILES]
                for p in sorted(by_file[rel])) + "\n")
    return "".join(out)

def gate(root: Path) -> tuple[int, str]:
    config = load_config(root)
    rel = f"{QUALITY_DIR}/{LEDGER_FILE}"
    try:
        staged = json.loads(git(root, "show", f":{rel}"))
    except (subprocess.CalledProcessError, json.JSONDecodeError):
        staged = {}
    blobs = indexed_blobs(root)
    # Repo-wide, not staged-only: a file that changed and then stopped being touched
    # is exactly the one a staged-set check never asks about again.
    owing = [f for f in tracked_in_scope(root, config["scope"])
             if owes_review(staged.get(f), blobs.get(f))]
    findings = []
    if owing:
        more = f" (+{len(owing) - 8} more)" if len(owing) > 8 else ""
        findings.append(f"[unreviewed] {len(owing)} file(s) not reviewed at the content"
                        f" being committed: {', '.join(owing[:8])}{more}")
    if stale := sorted(p.name for p, _ in read_payloads(root, (".unverified",))):
        findings.append(f"[open-round] {len(stale)} payload(s) still awaiting action:"
                        f" {', '.join(stale[:5])}")
    if (root / rel).is_file() and subprocess.run(
            ["git", "diff", "--quiet", "--", rel], cwd=root).returncode != 0:
        findings.append(f"[ledger-unstaged] {rel}: changed but not staged — the review"
                        " would be lost")
    if not findings:
        return 0, ""
    lines = ["swarm-gate report:", *(f"  {f}" for f in findings)]
    if config.get("mode") != "enforce":
        lines.append(f"swarm-gate: advisory only (mode={config.get('mode')}).")
        return 0, "\n".join(lines) + "\n"
    lines.append("swarm-gate: blocking (mode=enforce). Run /swarm-review.")
    return 1, "\n".join(lines) + "\n"


# ----------------------------------------------------------------------- cli

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="ledger.py", description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("log", help="finder records its findings for one file")
    p.add_argument("target")
    p.add_argument("--agent", required=True)
    p.add_argument("--findings", required=True, help="path to a JSON findings file")
    p = sub.add_parser("resolve", help="session marks each finding fixed or rejected")
    p.add_argument("payload")
    p.add_argument("--fixed", action="append", default=[], metavar="ID")
    p.add_argument("--reject", action="append", default=[], metavar="ID=REASON")
    p.add_argument("--escalate", action="append", default=[], metavar="ID")
    p = sub.add_parser("rule", help="verifier upholds or denies each rejection")
    p.add_argument("payload")
    p.add_argument("--verifier", required=True)
    p.add_argument("--uphold", action="append", default=[], metavar="ID=REASON")
    p.add_argument("--deny", action="append", default=[], metavar="ID=REASON")
    p.add_argument("--rule", action="append", default=[], metavar="ID=RULE",
                   help="propose a rule for the owner, on an upheld rejection")
    sub.add_parser("record", help="write the blob for files that came back clean")
    p = sub.add_parser("known", help="upheld rejections on a file, for a finder")
    p.add_argument("--file", required=True)
    p.add_argument("--agent", default=None)
    p = sub.add_parser("show", help="resolve a finding id to the finding")
    p.add_argument("--id", required=True)
    p = sub.add_parser("plan", help="what owes a review, with each file's finder set")
    p.add_argument("--all", action="store_true", dest="every")
    sub.add_parser("batches", help="verifier batches, by tier")
    sub.add_parser("gate", help="pre-commit check")
    return parser

def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    # Anchor the repo on whatever path this subcommand names; only the repo-wide
    # queries (plan, record, batches, gate, show) fall back to the caller's cwd.
    anchor = next((getattr(args, name) for name in ("target", "file", "payload")
                   if getattr(args, name, None)), None)
    root, now = repo_root(anchor), datetime.now(timezone.utc).isoformat()
    try:
        if args.cmd == "log":
            source = Path(args.findings)
            print(log(root, relative(root, args.target), args.agent,
                      json.loads(source.read_text(encoding="utf-8")), now))
            source.unlink(missing_ok=True)
        elif args.cmd == "resolve":
            path, payload = open_payload(args.payload)
            print(resolve(path, payload, set(args.fixed),
                          parse_pairs(args.reject, "reject"), set(args.escalate)))
        elif args.cmd == "rule":
            path, payload = open_payload(args.payload)
            print(rule(root, path, payload, parse_pairs(args.uphold, "uphold"),
                       parse_pairs(args.deny, "deny"), parse_pairs(args.rule, "rule"),
                       args.verifier, now))
        elif args.cmd == "record":
            print(record(root), end="")
        elif args.cmd == "known":
            print(known(root, relative(root, args.file), args.agent), end="")
        elif args.cmd == "show":
            if (hit := show(root, args.id)) is None:
                raise ValueError(f"no finding {args.id} in any payload or the ledger")
            print(f"found in {hit[0]}\n{json.dumps(hit[1], indent=2)}")
        elif args.cmd == "plan":
            print(plan(root, args.every), end="")
        elif args.cmd == "batches":
            print(batches(root), end="")
        elif args.cmd == "gate":
            code, out = gate(root)
            print(out, end="")
            return code
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
