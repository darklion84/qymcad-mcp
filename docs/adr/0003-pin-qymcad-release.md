# ADR 0003: Pin QymCAD to a release tag that matches the installed app

- Status: accepted (2026-10-04)

## Context
The `.qcad` format changes between QymCAD builds without backward compatibility, and loading does not check a
format version (FINDINGS F-012). A file written by one build may fail to open, or open subtly wrong, in another.

## Decision
- Depend on the QymCAD crates by git **tag** of a published release (the app is distributed only as release
  builds), never on `main`. Currently `v0.1.1-dev.20261026`.
- Record the version in `qymcad_engine::QYMCAD_VERSION`, report it in `initialize.serverInfo` and in
  `doc_info`, and warn at startup if the installed QymCAD.app reports a different version.
- Upgrades follow docs/UPGRADING.md and are gated by the golden tests.
- (2026-10-25) The server's own version is the QymCAD release it is built for (`0.1.1-dev.20261026`); a server
  fix for the same release adds a counter (`0.1.1-dev.20261026.1`, SemVer orders it after the base). Repository
  tags carry the same number with a `v`. A test keeps the workspace version and `QYMCAD_VERSION` in step.

## Consequences
- The server lags QymCAD `main` until the next release; fixes from `main` are not available earlier.
- Users must keep the app and the server on the same release.
