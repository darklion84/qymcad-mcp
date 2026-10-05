# Security model

qymcad-mcp is a **local stdio server**: it runs as the user who started it, with that user's file access, and
talks only to the MCP client that spawned it. It opens no network ports.

## Threats we guard against
- **A confused or prompt-injected agent writing outside CAD files.** Tools that take paths accept only their
  own file types (`doc_open` / `doc_save`: `.qcad`; exports: their format's extension), so the server cannot be
  used to overwrite shell profiles, keys or source files. Relative paths resolve against the server's working
  directory.
- **Symlinks.** The target file and QymCAD's save companions (`.tmp~`, `.bak`) must not be symbolic links; a
  write through a planted link would land in an arbitrary file.
- **Optional confinement.** Set `QYMCAD_MCP_ROOT=/some/dir` in the server's environment to refuse any path
  outside that directory (symlinks are resolved before the check).
- **Overwriting a full document with an empty one** is refused by QymCAD's guarded save.
- **Expressions that crash the evaluator.** QymCAD's expression parser recurses without a depth cap; an agent
  string of 100 000 `(` overflowed the stack and killed the server. Every expression is checked first: at most
  1000 characters and 64 levels of parentheses, `^` or consecutive signs (`value::check_expr`, FINDINGS F-3B-9).

## Not in scope
- Malicious `.qcad` files: they are parsed by QymCAD's own loader (serde/RON, OCCT B-rep). Open only files
  you trust, as with the QymCAD app itself.
- Resource exhaustion by huge models: the server is single-user and local.

Report security issues privately via GitHub security advisories of this repository.
