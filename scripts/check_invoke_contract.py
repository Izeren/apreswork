#!/usr/bin/env python3
# Copyright 2026 Aleksandr Iushmanov (@izeren)
# SPDX-License-Identifier: Apache-2.0
"""Check every frontend `invoke` against the Rust command it names.

`src/lib/api.ts` is sixty-odd one-line wrappers around `invoke('name', {args})`, and
no test exercises any of them: the frontend suite injects doubles for the api module,
and the Rust suite calls the command bodies directly. The command name and the
argument keys are therefore the one seam in the stack that nothing covers — rename a
command or a parameter and every test stays green while the app fails in the user's
hands.

This closes that seam statically:

  1. every invoked name is registered in `generate_handler!`, and every registered
     name has a `#[tauri::command]` behind it;
  2. every argument key maps to a parameter of that command under the camelCase ->
     snake_case conversion Tauri itself performs, and every non-`Option` parameter is
     supplied;
  3. `invoke` is imported only by `api.ts` — otherwise the call sites checked here
     would not be all of them, and the first two checks would prove nothing.

Injected parameters (`State`, `AppHandle`, `Window`, `Channel`) come from Tauri, not
from the caller, and are skipped.

Run: python3 scripts/check_invoke_contract.py
"""

from __future__ import annotations

import argparse
import difflib
import re
import sys
from pathlib import Path

API_FILE = "src/lib/api.ts"
HANDLER_FILE = "src-tauri/src/lib.rs"
COMMAND_GLOB = "src-tauri/src/commands/*.rs"
TAURI_CORE = "@tauri-apps/api/core"

# Types Tauri fills in itself; they never appear in the invoke payload.
INJECTED = ("State<", "AppHandle", "Window", "Channel<")

INVOKE_RE = re.compile(r"\binvoke\s*\(")
COMMAND_RE = re.compile(r"#\[tauri::command[^\]]*\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*")
HANDLER_RE = re.compile(r"generate_handler!\s*\[")
IMPORT_RE = re.compile(r"import\s*\{([^}]*)\}\s*from\s*['\"]" + re.escape(TAURI_CORE))

# --- parsing helpers


JS_QUOTES = "\"'`"
# Rust's `'` opens a lifetime far more often than a char literal, and `State<'_, T>`
# appears in every command signature — treating it as a string swallows the rest.
RUST_QUOTES = '"'


def matching(text: str, start: int, opener: str, closer: str, quotes: str = JS_QUOTES) -> int:
    """Index just past the bracket that closes the one at `start`.

    Hand-rolled rather than regex because these payloads nest — `{ id, input: {
    content } }` and `State<'_, ActiveState>` both defeat a non-recursive pattern.
    """
    depth, i, quote = 0, start, ""
    while i < len(text):
        ch = text[i]
        if quote:
            if ch == "\\":
                i += 2
                continue
            if ch == quote:
                quote = ""
        elif ch in quotes:
            quote = ch
        elif ch == opener:
            depth += 1
        elif ch == closer:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError(f"unbalanced {opener!r} at offset {start}")


def split_top(text: str, brackets: str, quotes: str = JS_QUOTES) -> list[str]:
    """Split on commas outside every bracket pair in `brackets` and outside strings."""
    openers, closers = brackets[0::2], brackets[1::2]
    parts: list[str] = []
    buf: list[str] = []
    depth, i, quote = 0, 0, ""
    while i < len(text):
        ch = text[i]
        if quote:
            if ch == "\\":
                buf.append(text[i : i + 2])
                i += 2
                continue
            if ch == quote:
                quote = ""
            buf.append(ch)
        elif ch in quotes:
            quote = ch
            buf.append(ch)
        else:
            if ch in openers:
                depth += 1
            elif ch in closers:
                depth -= 1
            if ch == "," and depth == 0:
                parts.append("".join(buf))
                buf = []
            else:
                buf.append(ch)
        i += 1
    parts.append("".join(buf))
    return [p.strip() for p in parts if p.strip()]


def snake(name: str) -> str:
    """camelCase -> snake_case, matching Tauri's own argument-name conversion."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


# --- the two sides


def invocations(text: str) -> list[tuple[str, set[str] | None, int]]:
    """(command name, argument keys or None, line number) for each `invoke(...)`."""
    found = []
    for match in INVOKE_RE.finditer(text):
        open_paren = text.index("(", match.start())
        close = matching(text, open_paren, "(", ")")
        args = split_top(text[open_paren + 1 : close], "()[]{}")
        if not args or args[0][0] not in "\"'":
            continue  # a local helper also called `invoke`, not the Tauri one
        name = args[0][1:-1]
        keys = None
        if len(args) > 1 and args[1].startswith("{"):
            body = split_top(args[1][1:-1], "()[]{}")
            keys = {split_top(p, "()[]{}")[0].split(":")[0].strip() for p in body}
        found.append((name, keys, text.count("\n", 0, match.start()) + 1))
    return found


def commands(paths: list[Path]) -> dict[str, list[tuple[str, bool]]]:
    """{command name: [(parameter name, is optional)]}, injected parameters dropped."""
    out: dict[str, list[tuple[str, bool]]] = {}
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for match in COMMAND_RE.finditer(text):
            # `pub fn profile_status<R: tauri::Runtime>(app: ...)` — step over the
            # generic list, whose `(` would otherwise never be reached.
            cursor = match.end()
            if text[cursor : cursor + 1] == "<":
                cursor = matching(text, cursor, "<", ">", RUST_QUOTES) + 1
            open_paren = text.index("(", cursor)
            close = matching(text, open_paren, "(", ")", RUST_QUOTES)
            params = []
            for param in split_top(text[open_paren + 1 : close], "()[]{}<>", RUST_QUOTES):
                # First single colon: `active: tauri::State<..>` must not split on `::`.
                colon = next(
                    (
                        i
                        for i, ch in enumerate(param)
                        if ch == ":"
                        and param[i - 1 : i] != ":"
                        and param[i + 1 : i + 2] != ":"
                    ),
                    -1,
                )
                if colon < 0:
                    continue  # `self`, or a pattern this check has no business parsing
                name = param[:colon].replace("mut ", "").strip()
                rust_type = param[colon + 1 :].strip()
                if any(marker in rust_type for marker in INJECTED):
                    continue
                params.append((name, rust_type.startswith("Option<")))
            out[match.group(1)] = params
    return out


def registered(text: str) -> set[str]:
    """Command names inside `generate_handler![...]`, stripped of their module path."""
    match = HANDLER_RE.search(text)
    if not match:
        return set()
    open_bracket = text.index("[", match.start())
    close = matching(text, open_bracket, "[", "]", RUST_QUOTES)
    body = re.sub(r"//[^\n]*", "", text[open_bracket + 1 : close])
    return {entry.split("::")[-1].strip() for entry in body.split(",") if entry.strip()}


# --- checks


def check(root: Path) -> list[str]:
    problems: list[str] = []
    api = root / API_FILE
    calls = invocations(api.read_text(encoding="utf-8"))
    handler = registered((root / HANDLER_FILE).read_text(encoding="utf-8"))
    defined = commands(sorted(root.glob(COMMAND_GLOB)))

    for name in sorted(handler - set(defined)):
        problems.append(f"{HANDLER_FILE}: {name} is registered but has no #[tauri::command]")

    for name, keys, line in calls:
        where = f"{API_FILE}:{line}"
        if name not in handler:
            near = difflib.get_close_matches(name, handler, n=3)
            problems.append(
                f"{where}: invoke('{name}') names no registered command"
                + (f" — did you mean {' or '.join(near)}?" if near else "")
            )
            continue
        params = defined.get(name)
        if params is None or keys is None:
            continue  # unregistered-but-defined is caught above; no payload, nothing to match
        sent = {snake(key) for key in keys}
        expected = {param for param, _ in params}
        for extra in sorted(sent - expected):
            problems.append(
                f"{where}: invoke('{name}') sends '{extra}', which {name} does not take"
                f" (takes: {', '.join(sorted(expected)) or 'nothing'})"
            )
        for param, optional in params:
            if param not in sent and not optional:
                problems.append(
                    f"{where}: invoke('{name}') omits required parameter '{param}'"
                )

    for path in sorted(root.glob("src/**/*.ts")) + sorted(root.glob("src/**/*.svelte")):
        if path == api:
            continue
        for match in IMPORT_RE.finditer(path.read_text(encoding="utf-8")):
            if any(part.strip().split(" as ")[0] == "invoke" for part in match.group(1).split(",")):
                problems.append(
                    f"{path.relative_to(root)}: imports invoke from {TAURI_CORE}."
                    f" Route Tauri calls through {API_FILE} so this check covers them."
                )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    problems = check(args.root)
    for problem in problems:
        print(problem)
    if problems:
        print(f"\n{len(problems)} invoke-contract problem(s).")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
