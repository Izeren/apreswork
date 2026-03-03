#!/usr/bin/env python3
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0

import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import ledger as L

ISO = "2026-07-24T12:00:00+00:00"
LATER = "2026-07-25T12:00:00+00:00"
# scripts/swarm/test_swarm.py -> repo root is two levels up.
REPO_ROOT = Path(__file__).resolve().parents[2]
FINDERS = {
    "test": {"match": ["**/*.test.ts", "src-tauri/**/tests/**/*.rs"],
             "agents": ["code-smell-nitpicker", "doc-slop-reviewer",
                        "test-param-enforcer"]},
    "production": {"match": ["src/**", "src-tauri/src/**"],
                   "agents": ["code-smell-nitpicker", "doc-slop-reviewer"]}}


def make_repo(tmp: Path, mode="warn") -> Path:
    subprocess.run(["git", "init", "-q"], cwd=tmp, check=True)
    subprocess.run(["git", "config", "user.email", "t@test"], cwd=tmp, check=True)
    subprocess.run(["git", "config", "user.name", "t"], cwd=tmp, check=True)
    (tmp / ".quality").mkdir()
    (tmp / ".quality" / "gate.json").write_text(json.dumps({
        "mode": mode,
        "scope": ["**/*.test.ts", "src-tauri/**/tests/**/*.rs"],
        "finders": FINDERS}), encoding="utf-8")
    return tmp


class RepoCase(unittest.TestCase):
    MODE = "warn"

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = make_repo(Path(self._tmp.name), self.MODE)

    def tearDown(self):
        self._tmp.cleanup()

    def add(self, rel="a.test.ts", body="x\n") -> str:
        (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
        (self.root / rel).write_text(body, encoding="utf-8")
        subprocess.run(["git", "add", "--", rel], cwd=self.root, check=True)
        return rel

    def log(self, rel, agent, findings, now=ISO) -> Path:
        L.log(self.root, rel, agent, {"findings": findings}, now)
        state = "unverified" if findings else "verified"
        return L.payload_path(self.root, agent, rel, state)

    def write_ledger(self, data: dict) -> None:
        (self.root / ".quality" / "ledger.json").write_text(
            json.dumps(data, indent=2), encoding="utf-8")

    def stage_ledger(self, data: dict) -> None:
        self.write_ledger(data)
        subprocess.run(["git", "add", "--", ".quality/ledger.json"],
                       cwd=self.root, check=True)


class ScopeTests(unittest.TestCase):
    """The scope this repo actually ships, not a copy of it.

    A glob is the only thing standing between a source file and no review at all:
    the gate asks for a review of a file if and only if the scope matches it, so a
    tree missing from these patterns is a tree nothing ever reviews. That gap is
    invisible — the gate passes quietly on the files it does not know about — so the
    assertion reads `.quality/gate.json` itself rather than a fixture that can drift.
    """

    CONFIG = L.load_config(REPO_ROOT)

    def test_production_and_test_code_are_both_in_scope(self):
        cases = [
            ("ts_prod", "src/lib/api.ts", True),
            ("ts_test", "src/lib/api.test.ts", True),
            ("ts_entrypoint", "src/main.ts", True),
            ("svelte", "src/lib/components/tasks/TaskRow.svelte", True),
            ("rust_prod", "src-tauri/src/scheduler/engine.rs", True),
            ("rust_entrypoint", "src-tauri/src/main.rs", True),
            ("rust_tests_dir", "src-tauri/src/scheduler/engine/tests/place.rs", True),
            # Not source under review: build config, tooling, and prose.
            ("build_config", "vite.config.ts", False),
            ("tooling", "scripts/swarm/ledger.py", False),
            ("doc", "docs/quality/design.md", False),
        ]
        for name, path, expected in cases:
            with self.subTest(name):
                self.assertEqual(L.in_scope(path, self.CONFIG["scope"]), expected)

    def test_scope_covers_every_tracked_source_file(self):
        """No tracked file under `src/` or `src-tauri/src/` escapes the gate.

        The path cases above pin the shape of the globs; this pins their reach. A new
        source extension or a new tree arrives unreviewed and otherwise silent.
        """
        tracked = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "ls-files", "src", "src-tauri/src"],
            capture_output=True, text=True, check=True).stdout.split()
        source = [f for f in tracked
                  if Path(f).suffix in (".ts", ".js", ".svelte", ".rs")]
        self.assertTrue(source, "expected tracked source files to measure")
        missed = [f for f in source if not L.in_scope(f, self.CONFIG["scope"])]
        self.assertEqual(missed, [])

    def test_every_in_scope_file_has_a_finder_set(self):
        """A scope glob with no matching finder class is a file the gate demands a
        review of and nothing can ever review."""
        for path in ("src/lib/api.ts", "src/lib/api.test.ts", "src/main.ts",
                     "src/lib/components/tasks/TaskRow.svelte",
                     "src-tauri/src/scheduler/engine.rs",
                     "src-tauri/src/scheduler/engine/tests/place.rs"):
            with self.subTest(path):
                self.assertTrue(L.finders_for(path, self.CONFIG))


class FinderSetTests(unittest.TestCase):
    CONFIG = {"finders": FINDERS}

    def test_test_files_get_three_finders(self):
        self.assertEqual(L.finders_for("src/lib/a.test.ts", self.CONFIG),
                         ["code-smell-nitpicker", "doc-slop-reviewer",
                          "test-param-enforcer"])

    def test_production_files_get_two(self):
        self.assertEqual(L.finders_for("src/lib/a.ts", self.CONFIG),
                         ["code-smell-nitpicker", "doc-slop-reviewer"])

    def test_first_matching_class_wins(self):
        # a.test.ts matches BOTH classes; declaration order decides, so a test file
        # never silently loses test-param-enforcer.
        self.assertIn("test-param-enforcer",
                      L.finders_for("src/a.test.ts", self.CONFIG))

    def test_unmatched_file_has_no_finders(self):
        self.assertEqual(L.finders_for("docs/readme.md", self.CONFIG), [])


class NamingTests(unittest.TestCase):
    def test_payload_path(self):
        self.assertEqual(
            str(L.payload_path(Path("/repo"), "doc-slop-reviewer",
                               "src/lib/utils.test.ts", "verified")),
            "/repo/.swarm/doc-slop-reviewer.src__lib__utils.test.ts.verified")


class GitHelperTests(RepoCase):
    def test_write_blob_tracks_content(self):
        target = self.root / "a.test.ts"
        target.write_text("\n".join(f"line {i}" for i in range(100)) + "\n")
        old = L.write_blob(self.root, "a.test.ts")
        self.assertEqual(L.write_blob(self.root, "a.test.ts"), old)
        lines = target.read_text().splitlines()
        lines[0] = "changed 0"
        target.write_text("\n".join(lines) + "\n")
        self.assertNotEqual(L.write_blob(self.root, "a.test.ts"), old)

    def test_owes_review_on_any_difference(self):
        """One changed line is as unreviewed as a hundred — no tolerance band."""
        cases = [("never_reviewed", None, "abc", True),
                 ("findings_but_no_blob", {"findings": {}}, "abc", True),
                 ("same_content", {"blob": "abc"}, "abc", False),
                 ("one_line_moved", {"blob": "abc"}, "def", True),
                 ("not_in_index", {"blob": "abc"}, None, False)]
        for name, entry, blob, expected in cases:
            with self.subTest(name):
                self.assertEqual(L.owes_review(entry, blob), expected)

    def test_indexed_blobs_reads_every_tracked_path_at_once(self):
        (self.root / "a.test.ts").write_text("x\n")
        self.assertEqual(L.indexed_blobs(self.root), {})
        self.add()
        self.assertEqual(L.indexed_blobs(self.root),
                         {"a.test.ts": L.write_blob(self.root, "a.test.ts")})

    def test_indexed_blobs_survives_a_path_needing_quotes(self):
        """`git ls-files` C-quotes odd paths unless asked for NUL separation.

        A quoted key matches nothing in the ledger, so the file would read as never
        reviewed and block every commit with no way to clear it.
        """
        self.add("a b\"c.test.ts")
        self.assertIn("a b\"c.test.ts", L.indexed_blobs(self.root))

    def test_worktree_blobs_reads_disk_not_the_index(self):
        self.add()
        staged = L.indexed_blobs(self.root)["a.test.ts"]
        (self.root / "a.test.ts").write_text("y\n")
        self.assertNotEqual(L.worktree_blobs(self.root, ["a.test.ts"])["a.test.ts"],
                            staged)

    def test_relative_refuses_a_path_outside_the_root(self):
        """Failing open is worse here than failing loudly.

        An absolute path that survives into `payload_path` becomes a filename the repo
        has no file for, so `record` counts one finder fewer than reported and the file
        can never clear its set.
        """
        root = self.root.resolve()
        self.assertEqual(L.relative(root, str(root / "a.test.ts")), "a.test.ts")
        with self.assertRaises(ValueError):
            L.relative(root, str(root.parent / "outside.test.ts"))

    def test_update_ledger_reads_what_the_previous_write_left(self):
        L.update_ledger(self.root, lambda led: led.setdefault("a", {}).update(x=1))
        L.update_ledger(self.root, lambda led: led.setdefault("b", {}).update(y=2))
        self.assertEqual(L.load_ledger(self.root), {"a": {"x": 1}, "b": {"y": 2}})


class LogTests(RepoCase):
    def setUp(self):
        super().setUp()
        self.add()

    def test_minor_only_still_routes_to_verification(self):
        """An unjudged MINOR is indistinguishable from a false positive."""
        path = self.log("a.test.ts", "doc-slop-reviewer",
                        [{"severity": "MINOR", "summary": "vague comment"}])
        self.assertTrue(path.name.endswith(".unverified"))

    def test_a_clean_run_removes_a_stale_unverified_payload(self):
        stale = self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "s"}])
        clean = self.log("a.test.ts", "doc-slop-reviewer", [])
        self.assertFalse(stale.exists())
        self.assertTrue(clean.exists())

    def test_a_finding_removes_a_stale_verified_payload(self):
        clean = self.log("a.test.ts", "doc-slop-reviewer", [])
        dirty = self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "s"}])
        self.assertFalse(clean.exists())
        self.assertTrue(dirty.exists())

    def test_normalize_defaults(self):
        item = L.normalize({"summary": "dup body"}, "doc-slop-reviewer")
        self.assertEqual(item["symbol"], L.FILE_SCOPE)
        self.assertEqual(item["severity"], "MINOR")
        self.assertEqual(item["count"], 1)
        self.assertEqual(item["status"], "unverified")
        self.assertEqual(len(item["id"]), 36)

    def test_normalize_keeps_symbol_and_reads_a_count_out_of_text(self):
        item = L.normalize({"summary": "s", "symbol": "renders()",
                            "severity": "major", "count": "3 sites"}, "x")
        self.assertEqual((item["symbol"], item["severity"], item["count"]),
                         ("renders()", "MAJOR", 3))

    def test_ids_are_unique_per_finding(self):
        path = self.log("a.test.ts", "doc-slop-reviewer",
                        [{"summary": "one"}, {"summary": "two"}])
        payload = json.loads(path.read_text())
        self.assertEqual(len({f["id"] for f in payload["findings"]}), 2)

    def test_group_count_severity_rule(self):
        cases = [("tests [a, b, c]", "MAJOR"), ("tests [a, b]", "MINOR")]
        for summary, expected in cases:
            with self.subTest(summary):
                self.assertEqual(
                    L.normalize({"summary": summary, "severity": "MINOR"},
                                L.GROUP_RULE_AGENT)["severity"], expected)

    def test_bad_payloads_raise(self):
        with self.assertRaises(ValueError):
            L.normalize({"summary": "  "}, "x")
        with self.assertRaises(ValueError):
            L.log(self.root, "a.test.ts", "x", {"findings": "nope"}, ISO)
        with self.assertRaisesRegex(ValueError, "finding 1"):
            L.log(self.root, "a.test.ts", "x",
                  {"findings": [{"summary": "ok"}, {"summary": ""}]}, ISO)


class ResolveTests(RepoCase):
    def setUp(self):
        super().setUp()
        self.add()
        self.path = self.log("a.test.ts", "doc-slop-reviewer",
                             [{"summary": "one"}, {"summary": "two"}])
        self.payload = json.loads(self.path.read_text())
        self.ids = [f["id"] for f in self.payload["findings"]]

    def resolve(self, fixed=(), rejected=None, escalated=()):
        return L.resolve(self.path, json.loads(self.path.read_text()), set(fixed),
                         dict(rejected or {}), set(escalated))

    def test_all_fixed_removes_the_payload(self):
        self.resolve(fixed=self.ids)
        self.assertFalse(self.path.exists())

    def test_a_rejection_keeps_the_payload_for_a_verifier(self):
        self.resolve(fixed=[self.ids[0]], rejected={self.ids[1]: "intentional"})
        after = json.loads(self.path.read_text())["findings"]
        self.assertEqual([f["status"] for f in after], ["fixed", "rejected"])
        self.assertEqual(after[1]["reason"], "intentional")

    def test_escalation_is_recorded_only_on_the_rejection(self):
        self.resolve(fixed=[self.ids[0]], rejected={self.ids[1]: "r"},
                     escalated=[self.ids[1]])
        after = json.loads(self.path.read_text())["findings"]
        self.assertNotIn("escalate", after[0])
        self.assertTrue(after[1]["escalate"])

    def test_every_finding_needs_a_mark(self):
        with self.assertRaisesRegex(ValueError, "every finding needs a mark"):
            self.resolve(fixed=[self.ids[0]])

    def test_contradictions_and_unknown_ids_are_refused(self):
        cases = [
            ("both", {"fixed": self.ids, "rejected": {self.ids[0]: "r"}},
             "both fixed and rejected"),
            ("escalate_without_rejection",
             {"fixed": self.ids, "escalated": [self.ids[0]]},
             "only applies to a rejection"),
            ("unknown", {"fixed": [*self.ids, "nope"]}, "no such finding"),
        ]
        for name, kwargs, message in cases:
            with self.subTest(name), self.assertRaisesRegex(ValueError, message):
                self.resolve(**kwargs)


class RuleTests(RepoCase):
    def setUp(self):
        super().setUp()
        self.add()
        self.path = self.log("a.test.ts", "doc-slop-reviewer",
                             [{"summary": "one"}, {"summary": "two"}])
        self.ids = [f["id"] for f in json.loads(self.path.read_text())["findings"]]
        L.resolve(self.path, json.loads(self.path.read_text()), {self.ids[0]},
                  {self.ids[1]: "intentional"}, set())

    def rule(self, upheld=None, denied=None, rules=None):
        return L.rule(self.root, self.path, json.loads(self.path.read_text()),
                      dict(upheld or {}), dict(denied or {}), dict(rules or {}),
                      "middle-swarm-verifier", LATER)

    def test_uphold_writes_the_finding_into_the_ledger_keyed_by_id(self):
        self.rule(upheld={self.ids[1]: "the fixture owns the clock"})
        entry = L.load_ledger(self.root)["a.test.ts"]
        self.assertEqual(list(entry["findings"]), [self.ids[1]])
        record = entry["findings"][self.ids[1]]
        self.assertEqual(record["session_reason"], "intentional")
        self.assertEqual(record["verifier_reason"], "the fixture owns the clock")
        self.assertEqual(record["verifier"], "middle-swarm-verifier")
        self.assertNotIn("id", record, "the id is the key; storing it twice is drift")

    def test_uphold_never_sets_the_blob(self):
        """A file with rulings and no blob still owes a review — that is `record`'s
        question, and it is not answered until the whole finder set is clean."""
        self.rule(upheld={self.ids[1]: "r"})
        self.assertNotIn("blob", L.load_ledger(self.root)["a.test.ts"])

    def test_uphold_removes_the_finding_and_the_last_fixed_one_ends_the_round(self):
        self.rule(upheld={self.ids[1]: "r"})
        self.assertFalse(self.path.exists())

    def test_deny_keeps_both_arguments_and_returns_it_to_the_session(self):
        self.rule(denied={self.ids[1]: "the clock is read at module scope"})
        item = [f for f in json.loads(self.path.read_text())["findings"]
                if f["id"] == self.ids[1]][0]
        self.assertEqual(item["status"], "unverified")
        self.assertNotIn("reason", item)
        self.assertEqual([p["status"] for p in item["prior"]],
                         ["rejected", "denied"])
        self.assertEqual(item["prior"][0]["reason"], "intentional")
        self.assertEqual(L.load_ledger(self.root), {})

    def test_rejections_are_answered_in_full_and_fixed_ones_are_not_ruled_on(self):
        cases = [("partial", {}, "every rejection needs a ruling"),
                 ("fixed", {"upheld": {self.ids[0]: "r"}}, "not yours to rule on"),
                 ("both", {"upheld": {self.ids[1]: "r"},
                           "denied": {self.ids[1]: "r"}}, "both upheld and denied"),
                 ("unknown", {"upheld": {"nope": "r"}}, "no such finding")]
        for name, kwargs, message in cases:
            with self.subTest(name), self.assertRaisesRegex(ValueError, message):
                self.rule(**kwargs)

    def test_a_reason_is_required_on_both_sides(self):
        for flag in ("uphold", "deny"):
            with self.subTest(flag), self.assertRaises(ValueError):
                L.parse_pairs([f"{self.ids[1]}="], flag)

    def test_a_rule_proposal_rides_on_the_upheld_rejection(self):
        """No spool of its own: the proposal is only ever read next to the rejection
        that prompted it, and a separate file nothing reads back is a write-only sink."""
        self.rule(upheld={self.ids[1]: "r"},
                  rules={self.ids[1]: "fixtures own the clock"})
        record = L.load_ledger(self.root)["a.test.ts"]["findings"][self.ids[1]]
        self.assertEqual(record["rule"], "fixtures own the clock")
        self.assertIn("fixtures own the clock", L.known(self.root, "a.test.ts", None))

    def test_the_field_is_absent_when_no_rule_was_proposed(self):
        self.rule(upheld={self.ids[1]: "r"})
        self.assertNotIn(
            "rule", L.load_ledger(self.root)["a.test.ts"]["findings"][self.ids[1]])

    def test_a_rule_cannot_ride_on_a_denial(self):
        """A denied finding leaves no ledger record, so the proposal would vanish."""
        with self.assertRaisesRegex(ValueError, "upheld rejection"):
            self.rule(denied={self.ids[1]: "r"}, rules={self.ids[1]: "some rule"})


class RecordTests(RepoCase):
    ALL = ("code-smell-nitpicker", "doc-slop-reviewer", "test-param-enforcer")

    def clean_run(self, rel="a.test.ts", agents=ALL):
        for agent in agents:
            self.log(rel, agent, [])

    def test_a_full_clean_set_records_the_blob_and_an_empty_findings_map(self):
        self.add()
        self.clean_run()
        L.record(self.root)
        self.assertEqual(L.load_ledger(self.root),
                         {"a.test.ts": {"blob": L.write_blob(self.root, "a.test.ts"),
                                        "findings": {}}})

    def test_recording_consumes_the_payloads(self):
        self.add()
        self.clean_run()
        L.record(self.root)
        self.assertEqual(L.read_payloads(self.root), [])
        self.assertIn("recorded 0 file(s)", L.record(self.root))

    def test_a_partial_finder_set_is_refused(self):
        """"No open payload" is a different question from "every finder ran"."""
        self.add()
        self.clean_run(agents=self.ALL[:2])
        self.assertIn("missing test-param-enforcer", L.record(self.root))
        self.assertEqual(L.load_ledger(self.root), {})

    def test_an_open_payload_on_the_file_is_refused(self):
        self.add()
        self.clean_run(agents=self.ALL[:2])
        self.log("a.test.ts", "test-param-enforcer", [{"summary": "s"}])
        self.assertIn("awaiting action", L.record(self.root))
        self.assertEqual(L.load_ledger(self.root), {})

    def test_finders_disagreeing_on_the_blob_are_refused(self):
        """Re-running one finder after a fix used to compile an entry claiming the
        new content while carrying findings that described the old."""
        self.add()
        self.clean_run(agents=self.ALL[:2])
        (self.root / "a.test.ts").write_text("changed\n")
        self.clean_run(agents=self.ALL[2:])
        self.assertIn("disagree on the blob", L.record(self.root))
        self.assertEqual(L.load_ledger(self.root), {})

    def test_a_file_that_left_the_tree_is_skipped(self):
        self.add()
        self.clean_run()
        (self.root / "a.test.ts").unlink()
        self.assertIn("no longer on disk", L.record(self.root))

    def test_recording_leaves_upheld_rejections_in_place(self):
        self.add()
        self.write_ledger({"a.test.ts": {"findings": {"id-1": {"summary": "s"}}}})
        self.clean_run()
        L.record(self.root)
        entry = L.load_ledger(self.root)["a.test.ts"]
        self.assertEqual(list(entry["findings"]), ["id-1"])
        self.assertIn("blob", entry)


class QueryTests(RepoCase):
    def test_known_renders_the_argument_and_flags_a_stale_ruling(self):
        self.add()
        blob = L.write_blob(self.root, "a.test.ts")
        self.write_ledger({"a.test.ts": {"blob": blob, "findings": {
            "id-1": {"agent": "doc-slop-reviewer", "symbol": "renders()",
                     "severity": "MINOR", "summary": "vague comment",
                     "session_reason": "it names the invariant",
                     "verifier_reason": "the invariant is not obvious", "blob": blob},
            "id-2": {"agent": "code-smell-nitpicker", "symbol": "<file>",
                     "severity": "MAJOR", "summary": "old claim",
                     "blob": "stale"}}}})
        out = L.known(self.root, "a.test.ts", None)
        self.assertIn("it names the invariant", out)
        self.assertIn("the invariant is not obvious", out)
        self.assertNotIn("STALE", out.split("[id-2]")[0])
        self.assertIn("STALE", out.split("[id-2]")[1])

    def test_known_filters_by_agent_and_says_so_when_there_is_nothing(self):
        self.add()
        self.write_ledger({"a.test.ts": {"findings": {
            "id-1": {"agent": "doc-slop-reviewer", "summary": "s"}}}})
        self.assertIn("[id-1]", L.known(self.root, "a.test.ts", "doc-slop-reviewer"))
        self.assertIn("no upheld rejections",
                      L.known(self.root, "a.test.ts", "code-smell-nitpicker"))

    def test_show_finds_an_open_finding_then_falls_back_to_the_ledger(self):
        self.add()
        path = self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "open one"}])
        fid = json.loads(path.read_text())["findings"][0]["id"]
        self.write_ledger({"a.test.ts": {"findings": {"id-9": {"summary": "ruled"}}}})
        source, item = L.show(self.root, fid)
        self.assertIn("a.test.ts", source)
        self.assertEqual(item["summary"], "open one")
        source, item = L.show(self.root, "id-9")
        self.assertEqual(source, "ledger (a.test.ts)")
        self.assertEqual(item, {"id": "id-9", "summary": "ruled"})
        self.assertIsNone(L.show(self.root, "missing"))

    def test_plan_lists_what_owes_a_review_with_its_finder_set(self):
        self.add("a.test.ts")
        self.add("b.test.ts")
        self.write_ledger({"a.test.ts": {"blob": L.write_blob(self.root, "a.test.ts"),
                                         "findings": {}}})
        self.assertEqual(
            L.plan(self.root, False),
            "b.test.ts\tcode-smell-nitpicker,doc-slop-reviewer,test-param-enforcer\n")
        self.assertEqual(len(L.plan(self.root, True).splitlines()), 2)

    def test_plan_keeps_a_file_whose_round_is_still_open(self):
        """A reviewed-and-recorded blob does not mean the round is finished."""
        self.add()
        self.write_ledger({"a.test.ts": {"blob": L.write_blob(self.root, "a.test.ts"),
                                         "findings": {}}})
        self.assertEqual(L.plan(self.root, False), "")
        self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "s"}])
        self.assertIn("a.test.ts", L.plan(self.root, False))

    def test_plan_drops_a_file_once_its_content_matches_again(self):
        self.add()
        self.write_ledger({"a.test.ts": {"blob": "stale", "findings": {}}})
        self.assertIn("a.test.ts", L.plan(self.root, False))


class BatchTests(RepoCase):
    def test_an_escalated_file_goes_to_the_senior_tier(self):
        self.add("a.test.ts")
        self.add("b.test.ts")
        for rel, escalate in (("a.test.ts", False), ("b.test.ts", True)):
            path = self.log(rel, "doc-slop-reviewer", [{"summary": "s"}])
            payload = json.loads(path.read_text())
            fid = payload["findings"][0]["id"]
            L.resolve(path, payload, set(), {fid: "r"},
                      {fid} if escalate else set())
        lines = L.batches(self.root).splitlines()
        self.assertEqual(len(lines), 2)
        self.assertTrue(lines[0].startswith("middle\t"))
        self.assertIn("a.test.ts", lines[0])
        self.assertTrue(lines[1].startswith("senior\t"))
        self.assertIn("b.test.ts", lines[1])

    def test_one_escalated_finding_lifts_the_whole_file(self):
        """Tiering is per file, not per payload: a verifier reads the file once."""
        self.add()
        for agent, escalate in (("doc-slop-reviewer", False),
                                ("code-smell-nitpicker", True)):
            path = self.log("a.test.ts", agent, [{"summary": "s"}])
            payload = json.loads(path.read_text())
            fid = payload["findings"][0]["id"]
            L.resolve(path, payload, set(), {fid: "r"},
                      {fid} if escalate else set())
        lines = L.batches(self.root).splitlines()
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].startswith("senior\t"))
        self.assertEqual(len(lines[0].split("\t")[1].split()), 2)

    def test_batches_are_chunked_by_file(self):
        for i in range(L.BATCH_FILES + 1):
            rel = self.add(f"f{i}.test.ts")
            path = self.log(rel, "doc-slop-reviewer", [{"summary": "s"}])
            payload = json.loads(path.read_text())
            L.resolve(path, payload, set(),
                      {payload["findings"][0]["id"]: "r"}, set())
        lines = L.batches(self.root).splitlines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(len(lines[0].split("\t")[1].split()), L.BATCH_FILES)
        self.assertEqual(len(lines[1].split("\t")[1].split()), 1)

    def test_nothing_awaiting_a_verifier_produces_no_batches(self):
        self.add()
        self.log("a.test.ts", "doc-slop-reviewer", [])
        self.assertEqual(L.batches(self.root), "")


class GateTests(RepoCase):
    MODE = "enforce"

    def seed(self, rel="a.test.ts"):
        """A file staged and recorded as reviewed at exactly that content."""
        self.add(rel)
        self.stage_ledger({rel: {"blob": L.write_blob(self.root, rel),
                                 "findings": {}}})

    def test_an_unreviewed_file_blocks(self):
        self.add()
        code, out = L.gate(self.root)
        self.assertEqual(code, 1)
        self.assertIn("[unreviewed]", out)
        self.assertIn("a.test.ts", out)

    def test_a_reviewed_file_at_the_same_content_passes(self):
        self.seed()
        self.assertEqual(L.gate(self.root), (0, ""))

    def test_one_changed_line_blocks_again(self):
        self.seed()
        (self.root / "a.test.ts").write_text("changed\n")
        subprocess.run(["git", "add", "--", "a.test.ts"], cwd=self.root, check=True)
        self.assertEqual(L.gate(self.root)[0], 1)

    def test_an_untouched_file_is_still_checked(self):
        """Freshness is repo-wide: a file that changed and then stopped being touched
        is exactly the one a staged-set check never asks about again."""
        self.seed()
        self.add("b.test.ts")
        self.stage_ledger({"a.test.ts": {"blob": L.write_blob(self.root, "a.test.ts"),
                                         "findings": {}}})
        code, out = L.gate(self.root)
        self.assertEqual(code, 1)
        self.assertIn("b.test.ts", out)

    def test_an_open_payload_blocks(self):
        self.seed()
        self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "s"}])
        code, out = L.gate(self.root)
        self.assertEqual(code, 1)
        self.assertIn("[open-round]", out)

    def test_an_unstaged_ledger_blocks(self):
        """The review lives in the commit or it does not live at all."""
        self.seed()
        self.write_ledger({"a.test.ts": {"blob": "other", "findings": {}}})
        code, out = L.gate(self.root)
        self.assertEqual(code, 1)
        self.assertIn("[ledger-unstaged]", out)

    def test_only_the_staged_ledger_counts(self):
        """A review written to disk but left out of the commit does not clear it."""
        self.add()
        self.write_ledger({"a.test.ts": {"blob": L.write_blob(self.root, "a.test.ts"),
                                         "findings": {}}})
        self.assertIn("[unreviewed]", L.gate(self.root)[1])

    def test_upheld_rejections_do_not_block(self):
        """A ruled rejection is settled debt; only unreviewed content blocks."""
        self.add()
        self.stage_ledger({"a.test.ts": {
            "blob": L.write_blob(self.root, "a.test.ts"),
            "findings": {"id-1": {"summary": "s", "severity": "MAJOR"}}}})
        self.assertEqual(L.gate(self.root), (0, ""))

    def test_advisory_mode_reports_without_blocking(self):
        (self.root / ".quality" / "gate.json").write_text(json.dumps({
            "mode": "warn", "scope": ["**/*.test.ts"], "finders": FINDERS}))
        self.add()
        code, out = L.gate(self.root)
        self.assertEqual(code, 0)
        self.assertIn("advisory only", out)


class CliTests(RepoCase):
    def run_cli(self, *argv) -> tuple[int, str]:
        out = io.StringIO()
        cwd = os.getcwd()
        os.chdir(self.root)
        try:
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
                code = L.main(list(argv))
        finally:
            os.chdir(cwd)
        return code, out.getvalue()

    def test_a_finding_travels_from_finder_to_ledger_and_clears_the_gate(self):
        self.add()
        source = self.root / "findings.json"
        source.write_text(json.dumps({"findings": [
            {"summary": "clock read in a fixture", "symbol": "setup()",
             "severity": "MAJOR"}]}))
        code, out = self.run_cli("log", "a.test.ts", "--agent", "doc-slop-reviewer",
                                 "--findings", str(source))
        self.assertEqual(code, 0)
        self.assertIn("1 MAJOR", out)
        self.assertFalse(source.exists(), "the input is consumed, not left to re-log")

        path = L.payload_path(self.root, "doc-slop-reviewer", "a.test.ts",
                              "unverified")
        fid = json.loads(path.read_text())["findings"][0]["id"]
        self.assertEqual(self.run_cli("show", "--id", fid)[0], 0)
        self.assertEqual(
            self.run_cli("resolve", str(path), "--reject", f"{fid}=the fixture owns "
                         "the clock")[0], 0)
        self.assertTrue(self.run_cli("batches")[1].startswith("middle\t"))
        self.assertEqual(
            self.run_cli("rule", str(path), "--verifier", "middle-swarm-verifier",
                         "--uphold", f"{fid}=agreed, the fixture owns it")[0], 0)
        self.assertIn(fid, L.load_ledger(self.root)["a.test.ts"]["findings"])
        self.assertIn("do not raise these again",
                      self.run_cli("known", "--file", "a.test.ts")[1])

        for agent in ("code-smell-nitpicker", "test-param-enforcer",
                      "doc-slop-reviewer"):
            self.log("a.test.ts", agent, [])
        self.assertIn("recorded 1 file(s)", self.run_cli("record")[1])
        self.assertEqual(self.run_cli("plan")[1], "")
        subprocess.run(["git", "add", "--", ".quality/ledger.json"],
                       cwd=self.root, check=True)
        self.assertEqual(self.run_cli("gate")[0], 0)

    def test_errors_report_and_exit_nonzero(self):
        cases = [("missing_payload", ("resolve", "nope.unverified"),
                  "not an existing"),
                 ("bad_pair", ("rule", "nope.unverified", "--verifier", "v",
                               "--uphold", "no-equals"), "not an existing"),
                 ("unknown_id", ("show", "--id", "nope"), "no finding")]
        for name, argv, message in cases:
            with self.subTest(name):
                code, out = self.run_cli(*argv)
                self.assertEqual(code, 1)
                self.assertIn(message, out)

    def test_a_target_in_another_repo_anchors_on_that_repo(self):
        """A dispatched finder inherits the parent session's cwd, not the repo it reviews.

        Anchoring on cwd wrote the payload into the neighbouring repo under a mangled
        absolute name, where `record` never sees it — the file simply stays one finder
        short of its set with nothing on disk to say why.
        """
        self.add()
        with tempfile.TemporaryDirectory() as elsewhere:
            neighbour = make_repo(Path(elsewhere))
            source = neighbour / "findings.json"
            source.write_text(json.dumps({"findings": []}), encoding="utf-8")
            cwd = os.getcwd()
            os.chdir(neighbour)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    code = L.main(["log", str(self.root / "a.test.ts"),
                                   "--agent", "doc-slop-reviewer",
                                   "--findings", str(source)])
            finally:
                os.chdir(cwd)
            self.assertEqual(code, 0)
            self.assertFalse((neighbour / L.SWARM_DIR).exists(),
                             "the payload belongs to the repo under review")
        self.assertTrue(L.payload_path(self.root, "doc-slop-reviewer", "a.test.ts",
                                       "verified").is_file())

    def test_a_verifier_can_propose_a_rule_while_upholding(self):
        self.add()
        path = self.log("a.test.ts", "doc-slop-reviewer", [{"summary": "s"}])
        fid = json.loads(path.read_text())["findings"][0]["id"]
        L.resolve(path, json.loads(path.read_text()), set(), {fid: "r"}, set())
        code, _ = self.run_cli("rule", str(path), "--verifier", "v",
                               "--uphold", f"{fid}=agreed",
                               "--rule", f"{fid}=never read the clock in a fixture")
        self.assertEqual(code, 0)
        self.assertEqual(
            L.load_ledger(self.root)["a.test.ts"]["findings"][fid]["rule"],
            "never read the clock in a fixture")


if __name__ == "__main__":
    unittest.main(verbosity=2)
