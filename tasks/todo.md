# Usability U1–U7

Baseline: `phase4a/usability`, clean working tree; pinned crates and installed app both
`v0.1.0-dev.20261001`. No commits (sandbox cannot write `.git`).

Plan reviewed against the requested acceptance criteria before implementation. Independent items are
delegated under AGENTS.md; parent owns session lifecycle, integration, and final verification.

- [x] Read AGENTS, architecture, findings, backlog, live tester/MiMo reports; verify pin/app.
- [ ] U1: investigate native deletion/dependencies; fail-first delete/undo tests; implement atomic deletion
  and bounded session undo; mutation checks including exact B-rep restoration.
- [ ] U2: investigate feature/cut-tool patterns; implement only if native support exists; document options
  otherwise; clarify whole-body arrays and sketch-circle cut-through workaround.
- [ ] U3: fail-first concave/convex selection tests; implement supported kernel-derived classification;
  mutation check and explicit scope.
- [ ] U4: fail-first sealed/open pocket warning tests; native shell-count comparison; mutation check.
- [ ] U5: fail-first stale stored-B-rep open test; volume/bbox tolerance comparison; mutation check.
- [ ] U6: verified face-frame descriptions/reporting, extrude op names, directory requirement, useful axis
  point, bbox documentation; fail-first behavior tests and mutation checks.
- [ ] U7: datum dependency labels/docs, area tolerance derivation/one-sided test, both-mode coverage,
  literal rectangle explanation and precise evidence claims; fail-first tests and mutation checks.
- [ ] Integrate findings with source/test evidence, design ADR, CHANGELOG and architecture updates.
- [ ] Regenerate TOOLS; run `./scripts/check.sh`; record test count and per-item evidence in review-u1.

## Review

Pending; detailed item evidence is being accumulated in `tasks/review-u*.md`.
