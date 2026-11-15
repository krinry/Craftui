# Design notes

These documents were written before and during the first development of
CraftUI. They record intent, plans and an early issue list. They are kept for
context and are **not** maintained as a description of the current code.

Where a file here disagrees with the book in `docs/src/` or with the code, the
book and the code are right. In particular:

- Syntax shown in the specification may differ from the real `.cui` grammar.
  The reference is [the language guide](../src/dsl/syntax-guide.md).
- Items ticked in the issue register were recorded by the original author and
  have not all been re-checked. What is checked automatically is listed under
  "Status" in the [README](../../README.md).
- Size and speed figures are targets or one-off measurements, not results that
  CI reproduces.

| File | Contents |
|---|---|
| [`product-requirements.md`](product-requirements.md) | Goals, audience and required capabilities. |
| [`architecture.md`](architecture.md) | Planned crate structure and rendering pipeline. |
| [`dsl-and-design-spec.md`](dsl-and-design-spec.md) | Early specification of the `.cui` language and the design tokens. |
| [`roadmap-and-risks.md`](roadmap-and-risks.md) | Original phased plan and risk list. |
| [`issue-register.md`](issue-register.md) | Audit of bugs and gaps with the author's completion marks. |
| [`directions.md`](directions.md) | Working notes on next steps. |
