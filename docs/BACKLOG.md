# Backlog

Known improvements that are not scheduled yet. Each entry says why it matters and where to start.

- **Re-solve only the sketches a parameter edit reaches.** `Session::propagate_params` (crates/engine/src/params.rs)
  re-solves every sketch that has any expression on every parameter edit. Solving only the sketches whose
  expressions mention the changed name (directly or through other parameters) is a performance optimisation for
  large documents; it must keep F-001 (case-insensitive matching) and F-002 in mind. Raised in the phase 3A review.
