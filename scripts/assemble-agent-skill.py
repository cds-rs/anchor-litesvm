#!/usr/bin/env python3
"""Assemble the agent skill from the book's agents pages.

The pages under book/src/agents/ are the source of truth (built and
link-checked with the rest of the book; their code blocks are {{#include}}d
from CI-tested crates). This script renders them into the skill layout:

    build/agent-skill/
      SKILL.md            <- agents/index.md, includes resolved, + frontmatter
      references/<page>.md
      llms.txt            <- index of the assembled files

The skill directory is a build artifact: edit the book pages, never the
output. Run from the repo root: python3 scripts/assemble-agent-skill.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "book" / "src" / "agents"
OUT = ROOT / "build" / "agent-skill"
BOOK_URL = "https://github.com/cds-rs/anchor-litesvm/blob/turbin3/book/src"
SITE_URL = "https://cds-rs.github.io/anchor-litesvm"

INCLUDE = re.compile(r"\{\{#include\s+([^:}\s]+)(?::([\w-]+))?\}\}")
ANCHOR = re.compile(r"ANCHOR(?:_END)?:\s*[\w-]+")
REFERENCE_PAGES = {
    "writing-tests": "one canonical suite: world setup, cast list, act, assert",
    "dependencies": "manifests per consumer shape (Anchor, Pinocchio, cross-engine)",
    "backends": "the TestSVM matrix and a construction recipe per engine",
    "auditing": "security findings as byte-stable Reports; the two observability tiers",
    "failure-modes": "fixes indexed by literal error text",
}

FRONTMATTER = """---
name: anchor-litesvm-testing
description: >-
  Write and fix Solana program tests with anchor-litesvm/testsvm: idiomatic
  suites (world, actors, scenarios), dependency setup per consumer shape
  (Anchor, Pinocchio, cross-engine), backend selection (litesvm, mollusk,
  RPC surfnet), and an error-string failure index.
---

"""


def extract_anchor(text: str, name: str, path: Path) -> str:
    lines, keep, found = text.splitlines(), [], False
    inside = False
    for line in lines:
        if re.search(rf"ANCHOR:\s*{re.escape(name)}\b", line):
            inside, found = True, True
            continue
        if re.search(rf"ANCHOR_END:\s*{re.escape(name)}\b", line):
            inside = False
            continue
        if inside and not ANCHOR.search(line):
            keep.append(line)
    if not found:
        sys.exit(f"error: anchor '{name}' not found in {path}")
    return "\n".join(keep)


def resolve_includes(text: str, base: Path) -> str:
    def repl(m: re.Match) -> str:
        path = (base / m.group(1)).resolve()
        if not path.exists():
            sys.exit(f"error: include not found: {path}")
        body = path.read_text()
        if m.group(2):
            return extract_anchor(body, m.group(2), path)
        # Whole-file include: drop anchor marker lines, as mdBook does.
        return "\n".join(l for l in body.splitlines() if not ANCHOR.search(l))

    return INCLUDE.sub(repl, text)


def rewrite_links(text: str, to_references: bool) -> str:
    # Sibling corpus pages move under references/ in the skill layout.
    if to_references:
        for page in REFERENCE_PAGES:
            text = text.replace(f"]({page}.md)", f"](references/{page}.md)")
    # Links escaping the corpus point at the published book source.
    return re.sub(r"\]\(\.\./([\w./-]+\.md)\)", rf"]({BOOK_URL}/\1)", text)


def main() -> None:
    refs = OUT / "references"
    refs.mkdir(parents=True, exist_ok=True)

    index = resolve_includes((SRC / "index.md").read_text(), SRC)
    (OUT / "SKILL.md").write_text(FRONTMATTER + rewrite_links(index, True))

    for page in REFERENCE_PAGES:
        text = resolve_includes((SRC / f"{page}.md").read_text(), SRC)
        (refs / f"{page}.md").write_text(rewrite_links(text, False))

    # llms.txt format (llmstxt.org): absolute URLs, so the same file works at
    # the skill root and copied to the site root.
    (OUT / "llms.txt").write_text(
        "# anchor-litesvm agent corpus\n\n"
        "> Machine-first documentation for writing and fixing Solana program "
        "tests with anchor-litesvm/testsvm. Every code block is included from "
        "a compiling, CI-tested crate.\n\n"
        "## Corpus\n\n"
        f"- [Conventions, vocabulary, decision trees]({SITE_URL}/agent-skill/SKILL.md)\n"
        + "".join(
            f"- [{p}]({SITE_URL}/agent-skill/references/{p}.md): {desc}\n"
            for p, desc in REFERENCE_PAGES.items()
        )
        + "\n## Optional\n\n"
        f"- [The book]({SITE_URL}/): the same documentation rendered for humans\n"
    )
    print(f"assembled {OUT.relative_to(ROOT)} (SKILL.md + {len(REFERENCE_PAGES)} references + llms.txt)")


if __name__ == "__main__":
    main()
