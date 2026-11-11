# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`GLOSSARY.md`** at the repo root: the vocabulary this fork adds (gestures, recognition, pens, toolbars, views).
- **`docs/adr/`**: read the ADRs that touch the area you're about to work in.

If either is missing, **proceed silently**. Don't flag its absence; don't suggest creating it upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates entries lazily when terms or decisions actually get resolved.

## File structure

Single-context repo: one glossary and one ADR directory at the root.

```
/
├── GLOSSARY.md
├── docs/adr/
│   ├── 0001-gesture-layer.md
│   └── 0013-toolbar-layout-persistence.md
└── crates/
```

ADRs are numbered with four digits, one per decision, and link each other by filename (`[0004](0004-draw-and-hold-curves.md)`). Cite a decision as `ADR-0004` and link the file, so a renumber can't leave the reference ambiguous.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts [ADR-0007](../adr/0007-ellipse-fit-refinement.md) (the ellipse is fitted where the pen drew it), but worth reopening because…_
