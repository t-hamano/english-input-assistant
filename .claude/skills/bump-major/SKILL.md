---
name: bump-major
description: Bump the application's major version across manifests and lockfiles and commit (e.g., 0.1.0 to 1.0.0).
disable-model-invocation: true
---

Bump the major version of this application.

## Steps

1. Review `git status` and existing changes so unrelated work is not included in the version commit.
2. Read the current version from `src-tauri/tauri.conf.json` and parse it as `major.minor.patch`. If it contains a prerelease or build suffix, clarify the intended next version before changing it.
3. Increment major by 1 and reset minor and patch to 0 (e.g., `0.1.0` -> `1.0.0`).
4. Set the new application version in all four files:
   - `src-tauri/tauri.conf.json`: `version`.
   - `package.json`: `version`.
   - `src-tauri/Cargo.toml`: `[package].version`.
   - `src-tauri/Cargo.lock`: `version` in the package named `english-input-assistant`.
5. Run `pnpm check-version` to verify that all application versions match and review the diff. Preserve dependency versions and unrelated changes; do not regenerate lockfiles just to change the application version. `pnpm-lock.yaml` does not store the application's version and needs no change for a version-only bump.
6. Stage only the version changes and commit with the title `Bump version to <new-version>`. Do not create a tag or push unless the user requests it.
