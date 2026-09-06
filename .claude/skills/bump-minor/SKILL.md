---
name: bump-minor
description: Bump the minor version across all config files and commit (e.g., 0.1.1 -> 0.2.0)
disable-model-invocation: true
allowed-tools: Read Edit Bash(git *) Bash(cargo generate-lockfile*)
---

Bump the minor version of the application.

## Steps

1. Read the current version from `src-tauri/tauri.conf.json` (the `version` field).
2. Parse the version as `major.minor.patch` (semver).
3. Calculate the new version: increment minor, reset patch to 0 (e.g., `0.1.1` -> `0.2.0`).
4. Update the `version` field in all three files:
   - `src-tauri/tauri.conf.json`
   - `package.json`
   - `src-tauri/Cargo.toml`
5. Run `cargo generate-lockfile` in `src-tauri/` to update `Cargo.lock`.
6. Commit all changed files with the message: `Bump version to <new-version>`
