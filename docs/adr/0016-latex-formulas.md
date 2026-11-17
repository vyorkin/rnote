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

Paste and commit recognize two display delimiters, `$$…$$` and `\[…\]`, and two inline ones,
`$…$` and `\(…\)`, each balanced with a non-empty body; an inline formula may not span a line
break. Everything else — unbalanced or empty delimiters, escaped `\$` — stays literal text; there is
no "looks like LaTeX" heuristic. Recognized math splits the insertion into separate objects: a
`TextStroke` for the text before it, the formula, a `TextStroke` for the text after it.

A display formula stacks above and below the text it was pasted with. An inline formula flows on the
line: text runs are measured with the same text style and the formula image is placed so its
baseline sits on the line's text baseline (`RenderedFormula.baseline` from the layout box). A box
containing an inline formula is therefore laid out line by line and keeps hard line breaks only, so
it no longer wraps; a display formula inside such a box still starts a block of its own.

A formula is a `VectorImage` whose SVG is the typeset output, plus a serde-defaulted
`FormulaSource` field holding the LaTeX source, its font size, color, inline flag, baseline and
height. The field is defaulted on read and ignored by older builds, which show the formula as a
plain SVG image; new builds can reopen it for editing. No `Stroke` variant is added, so the file
does not stop opening in older builds, and no format version bump is needed.

Rendering sits behind the `latex` compile-time feature, implemented with the pure-Rust RaTeX crates
(`ratex-parser`, `ratex-layout`, `ratex-svg`) and gated behind a spike that the generated SVG
renders through `librsvg`. Without the feature, recognized delimiters stay literal text. Editing a
formula shows its source — the box's edit mode is the source editor; clicking away, `Esc`, and
starting a new text box all re-render it, and a commit whose TeX fails to render is rejected and
restores the original object — and copies it as LaTeX `text/plain`, rendered SVG and the
stroke-content MIME.

## Considered Options

- **Inline formula runs inside `TextStroke`** — the Notion/Typora experience, and the reason it was
  rejected: rewriting the text box's model, cursor, selection and layout cache is a separate project
  from pasting formulas. Inline formulas are instead separate objects laid on measured text runs,
  which costs automatic wrapping inside a box that has one.
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
- A box with an inline formula is split into separate objects and loses automatic wrapping: its
  lines break only where the typed text has a newline. A single-line note, the common case, is
  unaffected.
- Editing an inline formula re-renders it on the baseline it had, so the surrounding text runs stay
  aligned; only a failure to typeset puts the original object back.
