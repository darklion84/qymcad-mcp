# Security model

qymcad-mcp is a **local stdio server**: it runs as the user who started it, with that user's file access, and
talks only to the MCP client that spawned it. It opens no network ports.

## Threats we guard against
- **A confused or prompt-injected agent writing outside CAD files.** Tools that take paths accept only their
  own file types (`doc_open` / `doc_save`: `.qcad`; `export`: only the chosen format's extensions — `.step`/`.stp`,
  `.stl`, `.3mf`, `.glb`, `.obj`), so the server cannot be used to overwrite shell profiles, keys or source files.
  An export overwrites an existing file of that type without asking. Relative paths resolve against the server's
  working directory. `render` writes no file: the image is returned inline.
- **Symlinks.** The target file and QymCAD's save companions (`.tmp~`, `.bak`) must not be symbolic links; a
  write through a planted link would land in an arbitrary file.
- **Hard links and late swaps.** Exports are written inside a private staging directory created fresh next to
  the target (mode 0700, `mkdtemp`-style: an existing entry of that name, a planted symlink included, is skipped,
  never followed), then the file is renamed over the target. A hard link, or a symlink swapped in after the
  check, never makes us truncate an unrelated file, and no other user can swap the temporary file while
  QymCAD's writers reopen it by name. `.qcad` saves use QymCAD's own temp-and-rename.
- **Optional confinement.** Set `QYMCAD_MCP_ROOT=/some/dir` in the server's environment to refuse any path
  outside that directory (symlinks are resolved before the check).
- **Regeneration errors anywhere in the document.** Export and render refuse when any timeline node has a
  regeneration error, even if the caller selects a clean body. This deliberately strict document-wide rule
  prevents output being mistaken for a successfully built model: failed QymCAD modifiers can pass their source
  shape through (FINDINGS F-008). Evidence: `golden_export.rs`
  `export_and_render_refuse_a_document_with_failed_features`.
- **Overwriting a full document with an empty one** is refused by QymCAD's guarded save.
- **Expressions that crash the evaluator.** QymCAD's expression parser recurses without a depth cap; an agent
  string of 100 000 `(` overflowed the stack and killed the server. Every expression is checked first: at most
  1000 characters and 64 levels of parentheses, `^` or consecutive signs (`value::check_expr`, FINDINGS F-3B-9).

- **Kernel output on the protocol channel.** OpenCASCADE prints to stdout (F-3C-1); the server moves fd 1 to
  stderr so nothing but JSON-RPC reaches the client (ADR 0005).

## Not in scope
- Malicious `.qcad` files: they are parsed by QymCAD's own loader (serde/RON, OCCT B-rep). Open only files
  you trust, as with the QymCAD app itself.
- Resource exhaustion by huge models: the server is single-user and local.
- Another process running as the same user, or anyone who can rename directories inside the target's parent
  while an export runs: such a process already has the user's write access to those files, and paths are
  resolved by name (the standard library has no `openat`/`renameat`). `QYMCAD_MCP_ROOT` confines what the
  agent asks for; it is not a sandbox against concurrent local processes.

Report security issues privately via GitHub security advisories of this repository.
