---
name: bump-patch
description: Bump the application's patch version across manifests and lockfiles and commit (e.g., 0.1.0 to 0.1.1).
---

Bump the patch version of this application, keeping the major and minor versions unchanged.

## Steps

1. Review `git status` and existing changes so unrelated work is not included in the version commit.
2. Read the current version from `src-tauri/tauri.conf.json` and parse it as `major.minor.patch`. If it contains a prerelease or build suffix, clarify the intended next version before changing it.
3. Increment patch by 1 (e.g., `0.1.0` -> `0.1.1`).
4. Set the new application version in all five files:
   - `src-tauri/tauri.conf.json`: `version`.
   - `package.json`: `version`.
   - `package-lock.json`: root `version` and `packages[""].version`.
   - `src-tauri/Cargo.toml`: `[package].version`.
   - `src-tauri/Cargo.lock`: `version` in the package named `english-input-assistant`.
5. Verify that all application versions match and review the diff. Preserve dependency versions and unrelated changes; do not regenerate lockfiles just to change the application version.
6. Stage only the version changes and commit with the title `Bump version to <new-version>`. Do not create a tag or push unless the user requests it.
