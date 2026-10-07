# MiMo review C1–C19

Plan checked against the clean branch and `/tmp/qm-review/codex-task-r2.md`.
Preserve prior commit 472d291. Verify each behavioral fix with a failing regression,
formula-derived expectations, server/GUI edits where relevant, and a reverted mutation.
Run check.sh before each item/group commit. Record exact evidence and refusals.

- [x] C1 correct through-hole changelog and B1 limit.
- [ ] C2 make rollback identity assertion precise (B-rep preferred).
- [ ] C3 compare topology rollback timeline ids/kinds.
- [ ] C4 refuse live edges without named kernel edges; reproduce if possible.
- [ ] C5 remove balanced singleton panic.
- [ ] C6 strictly validate edited array counts.
- [ ] C7 simplify positive finite predicate.
- [ ] C8 add formula-derived extrude variant goldens.
- [ ] C9 investigate reference flips; fix or refuse negative-x settled reference.
- [ ] C10 parenthesize negative turn subtraction.
- [ ] C11 golden shared-corner rectangle server/GUI coverage.
- [ ] C12 remove only temporary marker ids.
- [ ] C13 normalize Parallel satisfaction residual.
- [ ] C14 prune unreferenced angle helpers after constraint removal.
- [ ] C15 sign-crossing ADR.
- [ ] C16 world-space render bbox doc.
- [ ] C17 update architecture test descriptions.
- [ ] C18 document document-wide output error refusal.
- [ ] C19 selective deletion protection and endpoint dimension regression.

## Review

Pending. Per-item evidence will be retained in tasks/review-c.md, including commit,
test, pre-fix failure, mutation result, and any unproved scenario. Final gate:
`./scripts/check.sh`, with total passing test count.
