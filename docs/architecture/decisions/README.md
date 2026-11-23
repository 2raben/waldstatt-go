# Architectural decision records

ADRs explain lasting architectural choices and their tradeoffs. Read them alongside
the [current architecture](../README.md) and [contribution guide](../../../CONTRIBUTING.md).
Routine refactoring and individual implementation details do not need a record.

## Recording a decision

Copy [the template](_adr-template.md) to `NNNN-<slug>.md`, using the next four-digit
number starting at `0001`, and add a link and status to the index below. Include
Context, Decision, Consequences, and The tell.

Use `proposed` for unimplemented choices, `accepted` for implemented choices,
and `superseded` for replaced choices. Use the recording date; for retrospective
records add `recorded: true` and distinguish observed code from reconstructed
tradeoffs. Do not invent historical discussions or imply that a recording date is
the implementation date.

Preserve accepted rationale. For replacements, use `supersedes` / `superseded-by`
on the new and old records and mark the old record superseded. For partial changes,
use `amends` / `amended-by`, keep both records accepted, and explain the amendment
below the old title. Update both sides and the index.

Keep Markdown links and lineage paths relative to the ADR file. In `sources`,
code paths are repository-relative and document paths are ADR-relative. Remove
unused metadata and placeholders before accepting a record.

**The tell** identifies a repository-root search, focused test, or manual review
that can expose drift. State the expected result and limitations; a search alone
does not prove correctness.

## From the template

Waldstatt Go started from a Bevy template whose records described its starter
platformer. They were rewritten for this game: 0001 and 0002, on how groups move
and how the rules stay headless, were accepted when the board rules replaced the
platformer. 0003 holds as it was. The template's records remain
readable at [3b174ac](https://github.com/waldstatt-lab/waldstatt-go/tree/3b174ac/docs/architecture/decisions).

## Decision index

| Record | Status |
| --- | --- |
| [0001 — Move groups tile by tile on the board, whichever way it is viewed](0001-move-groups-tile-by-tile-on-the-board.md) | accepted |
| [0002 — Keep the rules headless and drive every rival through commands](0002-keep-the-rules-headless-behind-rival-commands.md) | accepted |
| [0003 — Pin the toolchain and run project tasks with mise](0003-run-project-tasks-with-mise.md) | accepted |
| [0004 — Compile the boards from copied Waldstatt water pages](0004-compile-the-boards-from-copied-waldstatt-water-pages.md) | accepted |
| [0005 — Build the same crate for the browser](0005-build-the-same-crate-for-the-browser.md) | accepted |
| [0006 — Paint the isometric sprites in code](0006-paint-the-isometric-sprites-in-code.md) | accepted |
| [0007 — Spread each group over the tiles of its Waldleute](0007-spread-each-group-over-the-tiles-of-its-waldleute.md) | accepted |
| [0008 — Lay the board in hexes](0008-lay-the-board-in-hexes.md) | accepted |
| [0009 — Measure computer rivals in a headless arena](0009-measure-computer-rivals-in-a-headless-arena.md) | accepted |
