# A pasted formula becomes a vector image with its LaTeX source

## Status

accepted (2026-11-17)

## Context

Pasting text that carries LaTeX math (`\[A=\{(1,0),(0,1)\}\]`) left the delimiters and the commands
as literal typewriter text. Rendering it needs three things the engine does not have: a delimiter
parser, a TeX typesetter, and a place on the canvas to hold the result.

The text box cannot host the result inline. `TextStroke` is a flat `String` with font-only ranged
attributes, and every geometry, hit-test, cursor and navigation call rebuilds one `piet` layout from
that string (`crates/rnote-engine/src/strokes/textstroke.rs`). Inline formula runs would mean
replacing the text box's model, cursor and layout cache — most of the feature's cost, for a workflow
the pasted text does not need.

## Decision

Paste and commit recognize exactly two balanced display delimiters, `$$…$$` and `\[…\]`. Everything
else — single `$…$`, `\(…\)`, unbalanced or empty delimiters — stays literal text; there is no
"looks like LaTeX" heuristic. Recognized math splits the insertion into separate objects: a
`TextStroke` for the text before it, the formula, a `TextStroke` for the text after it, each with its
own wrapping width.

A formula is a `VectorImage` whose SVG is the typeset output, plus a serde-defaulted
`FormulaSource` field holding the LaTeX source, its font size and color. Every recognized formula is
a display block, so no display flag is stored. Old readers ignore the unknown field and show the
formula as a plain SVG image; new readers can reopen it for editing. No `Stroke` variant is added,
so the file does not stop opening in older builds, and no format version bump is needed.

Rendering sits behind the `latex` compile-time feature, implemented with the pure-Rust RaTeX crates
(`ratex-parser`, `ratex-layout`, `ratex-svg`) and gated behind a spike that the generated SVG
renders through `librsvg`. Without the feature, recognized delimiters stay literal text. Editing a
formula shows its source — the box's edit mode is the source editor, commit re-renders, `Esc`
cancels by restoring the object, and a commit whose TeX fails to render is rejected and restores
the original object — and copies it as LaTeX `text/plain`, rendered SVG and the stroke-content
MIME.

## Considered Options

- **Inline formula runs inside `TextStroke`** — the Notion/Typora experience, and the reason it was
  rejected: rewriting the text box's model, cursor, selection and layout cache is a separate project
  from pasting formulas.
- **A new `Stroke::MathFormula` variant** — cleaner naming, but `Stroke` has no unknown-variant
  fallback, so every older build would fail to open the whole document instead of showing the
  formula as a picture.
- **An external `tectonic`/`pdflatex` process** — gold-standard TeX, but `tectonic` emits PDF only,
  the SVG step needs `dvisvgm`, and the first run fetches packages from the network; a formula must
  render offline. RaTeX keeps typesetting in-process and offline.
- **`katex-rs`** — mature, but it emits HTML and MathML, unusable without a webview.

## Consequences

- Formula layout is KaTeX-compatible math, not full LaTeX: `\usepackage`, `align` and sectioning are
  out of scope until the renderer changes.
- The formula is a picture to every other part of the engine: selection, movement, undo, PDF/SVG
  export (vector through `rsvg`) and XOPP work unchanged.
- A stale formula after a renderer upgrade is not re-rendered automatically; its stored SVG is what
  the document shows until the source is edited or re-rendered explicitly.
- Inline math remains unimplemented and is the known next step, not a regression.
