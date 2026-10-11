# Imported parts review log

Restoring imported STEP/IGES bodies on open (F-072, ADR 0019). The idea came from reading Ravva/qymcad-mcp,
which opens documents through `qymcad_testkit::open_like_the_app`. Implemented by OpenAI Codex (gpt-6.1-sol,
high reasoning) from a specification by Claude (Opus 5.5); reviewed by Xiaomi MiMo (MiMo-V2.6-Pro, read-only,
on snapshots) and Claude; live-tested through a real MCP client (Codex as tester).

| Round | Scope | MiMo | Claude | Live test | Outcome |
|---|---|---|---|---|---|
| 0 | investigation, then fix | — | check.sh 417 passed; removing the restoration call turns 9 of 11 new tests red | A/B, old vs new server: the old one silently dropped the imported cube (open, info, render, STEP export, after save/reopen) when the file held no B-rep for it; the new one restores it; no change for a file with stored B-reps | S1, S3, S4, S5 already passed; S2 was the bug |
| 1 | full change | ACCEPT; minors: import advisories other than source warnings could duplicate; a false "solid index absent" warning; nits 3–5 | accepted minors 1–2 and nit 5 | — | fixed by Codex (I1–I3) |
| 2 | fixes | ACCEPT, no new problems | check.sh 421 passed | PASS, including a corrupted embedded source: one warning naming the body, not repeated after rebuilds, the rest of the model keeps working | merged |

Deferred to docs/BACKLOG.md: the scale matrix is written out instead of reusing the GUI-private
`import_scale::scale_matrix` (identical values); a temporary source file is left behind if the staging closure
panics.
