# Hostile corpus

Documents that are trying to do something to the parser, the store, or a later
consumer. Each file exists because of a row in
`docs/threat-model/attack-catalog.csv`, and each is used by a test that asserts
the specific bad outcome does **not** happen.

These are inputs, not expected outputs. Nothing here is sanitised, and nothing
here should be "fixed" — a corrected hostile fixture tests nothing.

| File | Attack rows | What it must not do |
|---|---|---|
| `script-isolation.html` | A-009 | Markup written inside `<script>`, `<style>`, `<noscript>` or a comment must not become an observation. |
| `injection-strings.html` | A-005, A-009 | Instruction-shaped text and active markup must be stored inert and verbatim, never executed and never rewritten. |
| `csv-formula.html` | A-008 | Values beginning `=`, `+`, `-`, `@`, TAB or CR must survive extraction **unescaped**, so the escape happens once, at export. |
| `malformed.html` | A-012 | Unterminated tags, bad nesting and stray brackets must yield an error code or a partial reading, never a panic. |

The 500 MB document (A-011) is generated in the test rather than committed. A
half-gigabyte file in git would be paid for on every clone by everyone, forever,
to assert a property that a generator asserts just as well.
