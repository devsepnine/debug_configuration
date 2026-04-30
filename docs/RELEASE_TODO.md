# Release Packaging Follow-ups

This document tracks release work intentionally left after the initial GitHub Actions packaging setup.

## Completed Baseline

- GitHub Actions build workflow for Windows, Linux, and macOS.
- Manual draft release workflow using an existing tag.
- Release input validation for missing tags and duplicate releases.
- SHA-pinned GitHub Actions.
- Release checksums.
- Windows MSI installer packaging with WiX.
- macOS DMG packaging on macOS runners.
- Linux tarball packaging with desktop entry, icon, and install script.
- GPL-3.0 license metadata and `LICENSE` file.

## Remaining Work

### 1. Add Release Quality Gate

Mirror the `build.yml` quality job in `release.yml` before packaging:

- `cargo check --locked`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked`
- `cargo fmt --all -- --check`

Reason: manual releases should fail before OS packaging if the tagged source is not clean.

### 2. Improve macOS Distribution

- Add Apple code signing support.
- Add notarization support.
- Store signing credentials as GitHub Actions secrets.
- Consider a universal macOS binary if a single DMG is preferred over separate Intel and Apple Silicon DMGs.

Reason: unsigned DMGs work for development distribution, but users will see Gatekeeper warnings.

### 3. Improve Windows Installer Metadata

- Add a proper application icon.
- Add `ARPPRODUCTICON` for Windows Apps & Features.
- Add manufacturer/project URL metadata.
- Decide whether the installer should be per-user or per-machine.
- Add optional code signing for the MSI.

Reason: the current MSI is installable but still minimal.

### 4. Improve Linux Packaging

- Validate the release tarball on an Ubuntu runner after packaging.
- Consider adding AppImage.
- Consider Flatpak metadata if publishing to Flathub becomes a goal.
- Consider Snap only if there is a clear user need.

Reason: tarballs are useful for GitHub Releases, but Linux users often expect distro-style package channels.

### 5. Version and Release Notes

- Decide release versioning convention (`v0.1.0` or calendar-based).
- Add a changelog process.
- Consider generating release notes from commits or a curated changelog.

Reason: the current workflow can publish releases, but the release process is still lightweight.

### 6. Dependency and Security Checks

- Add `cargo audit` or equivalent advisory scanning.
- Add scheduled security workflow.
- Revisit action SHA pins periodically.

Reason: SHA pinning improves reproducibility, but dependency advisories still need separate coverage.
