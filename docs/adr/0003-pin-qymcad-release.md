# ADR 0003: Pin QymCAD to a release tag that matches the installed app

- Status: accepted (2026-10-04)

## Context
The `.qcad` format changes between QymCAD builds without backward compatibility, and loading does not check a
format version (FINDINGS F-012). A file written by one build may fail to open, or open subtly wrong, in another.

## Decision
- Depend on the QymCAD crates by git **tag** of a published release (the app is distributed only as release
  builds), never on `main`. Currently `v0.1.0-dev.20261001`.
- Record the version in `qymcad_engine::QYMCAD_VERSION`, report it in `initialize.serverInfo` and in
  `doc_info`, and warn at startup if the installed QymCAD.app reports a different version.
- Upgrades follow docs/UPGRADING.md and are gated by the golden tests.

## Consequences
- The server lags QymCAD `main` until the next release; fixes from `main` are not available earlier.
- Users must keep the app and the server on the same release.
