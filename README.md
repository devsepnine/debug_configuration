# Run/Debug Configuration Manager

**English** · [한국어](README_KR.md) · [日本語](README_JA.md)

A Run/Debug configuration management tool.
Built with Rust and the iced GUI framework, this cross-platform desktop application allows you to save and manage multiple program execution configurations.

![Preview](static/preview.gif)

## Download

Grab the latest pre-built binary for your platform from the [Releases page](https://github.com/devsepnine/debug_configuration/releases/latest), or use the direct links below:

| Platform | File |
|---|---|
| Windows x64 | [`run_config_manager-windows-x64.msi`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-windows-x64.msi) |
| macOS (Apple Silicon) | [`run_config_manager-macos-arm64.dmg`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-macos-arm64.dmg) |
| macOS (Intel) | [`run_config_manager-macos-x64.dmg`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-macos-x64.dmg) |
| Linux x64 | [`run_config_manager-linux-x64.tar.gz`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-linux-x64.tar.gz) |
| SHA-256 checksums | [`checksums.txt`](https://github.com/devsepnine/debug_configuration/releases/latest/download/checksums.txt) |

> macOS users: the .app bundle is ad-hoc signed (no paid Apple Developer ID). On first launch, see the [macOS — First Launch](#macos--first-launch) section for how to allow it.

## Key Features

### Configuration Management
- Create, edit, clone, and delete run configurations
- Select configuration type (Application, Shell Script, Node, Compound)
- Compound type runs several configurations together as one group
- Set command, arguments, and working directory
- Manage environment variables (add, edit, delete)
- Reorder configurations with drag and drop
- Open/save configurations (versioned JSON format)

### Node Project Support
- Node-oriented commands supported (run, install, start, test, build, etc.)
- Auto-detect Node Runtime (system PATH, nvm, nvm-windows)
- Auto-scan package.json and parse scripts
- Select or auto-detect package manager (npm, yarn, pnpm, bun)
- Async initialization for fast app startup

### Execution Session Management
- Run multiple sessions simultaneously
- Real-time output display (ANSI color support)
- Rerun, stop, remove, and hide sessions from a workspace
- Status badge on finished sessions (success, failed exit code, run duration)
- Desktop notification when a run finishes while the window is unfocused
- Bulk actions: Stop All, Rerun All, Rerun Failed
- Persistent session list with workspace-aware open state
- Workspace tab system
  - Keep at least one workspace tab available
  - Add, close, and rename workspaces
  - Open sessions into the selected workspace from the session list
- Pane-based workspace layout
  - Split panes with the iced pane grid
  - Drag and drop pane rearrangement
  - Resize panes
  - Maximize and restore panes when multiple panes are open

### Output Search and Export
- Search output (Ctrl+F): case-insensitive substring or regex
- Match-line highlighting with next/previous navigation and a match count
- Filter mode to show only matching lines
- Export the full session output to a text/log file

### UI Features
- Catppuccin Mocha theme
- D2Coding font
- Terminal-style output
- Custom window title bar and rounded window chrome
- Unified icon button styling across toolbar, sessions, and panes
- Open URLs in browser on click
- Clipboard copy support
- Auto-scroll functionality

## Screenshots

### Configuration Management Screen
![Configuration Management](static/config.png)

### Execution Sessions Screen
![Execution Sessions](static/session.png)

## Installation and Running

### Requirements
- Rust 1.85 or higher
- Cargo

### Build
```bash
cargo build --release
```

### Run
```bash
cargo run --release
```

### Linux runtime dependencies
On Linux the app relies on a few desktop services at runtime. They are present on a normal desktop session but may be missing on minimal/headless setups, in which case the related feature degrades silently:

- **File dialogs** (open / save / export output) use the XDG Desktop Portal over D-Bus — install and run `xdg-desktop-portal` plus a backend (`xdg-desktop-portal-gtk`, `-kde`, or `-hyprland`).
- **Desktop notifications** need a running notification daemon implementing `org.freedesktop.Notifications` (GNOME/KDE provide one; otherwise `dunst` / `mako`).
- **Opening URLs** uses `xdg-open` from `xdg-utils`.
- **Rendering** uses wgpu (Vulkan/GL); a working GPU driver (Mesa) is recommended (a tiny-skia software path is the fallback).

Build-time system libraries (for compiling from source) are listed in the CI workflow: `libfontconfig1-dev`, `libwayland-dev`, `libx11-xcb-dev`, `libxkbcommon-dev`, `pkg-config`.

### Platform Check
The project is intended to run on Windows, macOS, and Linux.

The following compile checks have been verified:
```bash
cargo check --target x86_64-pc-windows-msvc
cargo check --target x86_64-unknown-linux-gnu
cargo check --target x86_64-apple-darwin
cargo check --target aarch64-apple-darwin
```

Runtime behavior that depends on the OS compositor, such as transparent rounded window corners, should still be visually checked on each platform.

### Automated Release
GitHub Actions runs CI checks on pull requests and on pushes to `develop` and `release/**` branches.
Release artifacts are built for four platforms:
- Windows x64 — MSI installer
- Linux x64 — tarball with a desktop entry, icon, and install script
- macOS x64 — DMG disk image
- macOS arm64 — DMG disk image

To install a Linux release archive:
```bash
tar -xzf run_config_manager-linux-x64.tar.gz
cd run_config_manager-linux-x64
./install-linux.sh
```

#### Automatic release (recommended)
Push a `release/vX.Y.Z` branch to trigger an automatic build and a draft GitHub Release:
```bash
git checkout -b release/v0.2.0 develop
# update Cargo.toml version, finalize any last touches
git push -u origin release/v0.2.0
```
The workflow validates the version format (`vX.Y.Z` or `vX.Y.Z-suffix`), builds all four platforms, attaches a `checksums.txt`, and uploads them to a draft release.
Review the draft on the GitHub Releases page and click **Publish release** to make it public — the tag is created at the branch tip on publish.
Then merge `release/v0.2.0` back into `develop`.

#### Manual release (fallback)
If a tag already exists or you need to bypass the branch flow, push the tag first and then run the `Release` workflow manually from the GitHub Actions tab:
```bash
git tag v0.2.0 <commit>
git push origin v0.2.0
```

Either path stops if a release with the same tag already exists.
Run the `Checksum` workflow only when checksums need to be regenerated for an existing release.

### macOS — First Launch

The .app bundle is ad-hoc signed (no paid Apple Developer ID). On first launch macOS may show one of:
- `Apple could not verify "RunConfigManager" is free of malware...` (macOS 14+)
- `"RunConfigManager" is damaged and can't be opened` (when the quarantine attribute is active)

![macOS Gatekeeper dialog](static/done.png)

Two ways to allow it:

**Option 1 — System Settings (recommended)**
1. Click `Done` on the dialog to dismiss it.
2. Open **System Settings → Privacy & Security**.
3. Scroll to the Security section; you should see `"RunConfigManager" was blocked` with an **Open Anyway** button.

   ![Privacy & Security – Open Anyway](static/openanyway.png)

4. Click **Open Anyway** and authenticate with Touch ID or your password.
5. Launch RunConfigManager again — it will run from then on.

**Option 2 — Terminal (one shot)**
```bash
xattr -dr com.apple.quarantine /Applications/RunConfigManager.app
open /Applications/RunConfigManager.app
```

This removes the `com.apple.quarantine` attribute that triggers the Gatekeeper check, so the app launches directly without the dialog.

## How to Use

### 1. Create Configuration
1. Click the `Add` button in the `Configurations` tab
2. Set configuration name, type, command, etc.
3. Add environment variables if needed
4. Save with the `Save` button

### 2. Run Program
1. Select the configuration to run
2. Click the `Run` button or the play button in the configuration list
3. Automatically switches to the `Sessions` tab to view execution results

### 3. Pane Management
- **Open Session**: Click a session in the left session list to open it in the selected workspace
- **Hide Session**: Click the close button in a pane to remove it from the workspace without removing the running session
- **Stop/Remove Session**: Use the session list actions to stop or remove a session
- **Move Pane**: Drag the pane header to rearrange panes
- **Resize Pane**: Drag the pane border to resize
- **Maximize Pane**: Use the pane maximize button when the workspace has more than one pane

### 4. Open/Save Configurations
- **Open**: Open a JSON configuration file and replace the current configurations
- **Save**: Save configurations to the current file, or choose a file when no path is set

## Data Storage Location

Configuration files are automatically saved in the OS-specific settings directory:
- **Linux**: `~/.config/run_config_manager/configs.json`
- **macOS**: `~/Library/Application Support/run_config_manager/configs.json`
- **Windows**: `%APPDATA%\run_config_manager\configs.json`

## Shutdown Handling

The application automatically cleans up all running processes on exit:
1. Drop trait executes on Ctrl+C or window close
2. Send SIGTERM (or taskkill on Windows) to all sessions
3. Wait 2 seconds (provides graceful shutdown opportunity)
4. Force kill remaining processes (SIGKILL or taskkill /F)

## License

This project is licensed under the GNU General Public License v3.0. See [LICENSE](LICENSE) for details.
