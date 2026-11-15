# PlatformIO for OpenIDEA

An OpenIDEA plugin that brings [PlatformIO Core](https://platformio.org) into the IDE — build, upload, monitor, and manage boards and libraries for embedded projects without leaving the editor or relying on VS Code. It drives the real `pio` binary through a bottom-docked tool window, keyed terminals, and status bar items.

## Features

### Core setup
- **Install Python 3** — Homebrew on macOS, apt/dnf/pacman on Linux.
- **Install PlatformIO Core** — downloads the official installer into `~/.platformio`; reuses an existing install if present.
- **Show / Upgrade Core Version** — runs `pio --version` / `pio upgrade`.
- Runs Core through the system `python3` (on `PATH`), so only the `process:python3` and `process:sh` grants are needed — no path prompt.

### Daily project loop
- **Build**, **Upload**, **Clean**, **Test**, **Check** — run in keyed terminals against the approved `pio` binary, one reusable tab per operation.
- **Upload and Monitor** — uploads, then opens the serial monitor if the upload succeeds.
- **Monitor** — interactive serial monitor in its own terminal tab.
- **Full Clean**, **Run Target…**, **Run any `pio` command** — for everything Core can do.
- **Active environment switcher** — one click, remembered per project; every task passes `-e <env>` explicitly.

### Boards & libraries
- **Add Board** — append an environment to `platformio.ini` via `pio project init -b <board>` (searchable browser over `pio boards`).
- **Detect Board** — uses esptool to identify the connected chip.
- **Add Library** — search and install libraries via `pio lib search` / `pio lib install`.
- **Remove Environment** — delete an `[env:…]` section.

### Code intelligence
- **Update Code Index** — generates `compile_commands.json` (via `pio run -t compiledb`) so the IDE's C/C++ support resolves `#include <Arduino.h>` and library headers. Auto-runs on first view open when missing.

### Project management
- **Set Port** — pick a serial port from `pio device list` (or `auto`), stored per project.
- **New Project** — scaffold a project from a board + framework with sample code.
- **Terminal** — open a shell with `pio` on `PATH`.

## Requirements

- OpenIDEA `^1.4`
- Python 3 (installable from the plugin if missing)
- PlatformIO Core (installable from the plugin into `~/.platformio`)

## Installation

Build the plugin and link it into the IDE:

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```

`plugin.toml` declares the commands, so they appear in Find Action before the code is loaded. Capabilities the code needs are declared there too and approved on install.

## First run

1. If Python 3 is missing, run **PlatformIO: Install Python 3** first (Homebrew on macOS, apt/dnf/pacman on Linux).
2. Run **PlatformIO: Install PlatformIO Core**, which downloads the official installer into `~/.platformio`.
3. Approve `process:python3` (and `process:sh`) in Settings › Plugins if prompted.

The plugin runs Core through the system `python3` (on `PATH`), so only those two grants are needed.

## Usage

Feature testing needs a project with a `platformio.ini` open (e.g. `~/Documents/PlatformIO/Projects/c3bridge`). Open the **PlatformIO** tool window (bottom dock) — it lists every tool grouped by category and shows the active environment and status.

- Switch environments with the environment buttons or **Select Environment**.
- Build / Upload / Monitor from the toolbar buttons, status bar, or Find Action.
- Run **Update Code Index** (Code Index button) after building so IntelliSense resolves includes.

## Capabilities

The plugin requests these capabilities (approved on install):

| Capability | Purpose |
|---|---|
| `project.read` / `project.write` | Read/write project files (`platformio.ini`, index files) |
| `process:python3` | Run Core through the system Python |
| `process:sh` | Shell helpers (port detection, install) |
| `fs.read:~/.platformio` | Detect an existing Core install |
| `storage` | Persist active env, port, command history per project |

## Development

- Language: Rust, compiled to WebAssembly (`wasm32-wasip2`).
- SDK: `openidea-plugin` (pinned to tag `v0.1.45`).
- Pure logic is kept separate from host calls so it can be unit tested natively with `cargo test`.
- Manifest: `plugin.toml` (commands, views, file icons, capabilities).

## Notes

- The tool window icon uses the official PlatformIO mark copied from the VS Code extension. Before publishing anywhere, check PlatformIO's brand terms or swap in an original mark; the extension is Apache-2.0, which allows copying the file with attribution but grants no right to the PlatformIO trademark.
- `publish = false` — not published to a registry; install locally via `openidea-plugin link`.

## License

This plugin's own source code is licensed under the Apache License, Version 2.0
(see [LICENSE](LICENSE)). Upstream components retain their own licenses:
PlatformIO Core and its toolchains/packages are Apache-2.0, and the PlatformIO
mark copied into `icons/platformio.svg` is used with attribution under the
extension's Apache-2.0 terms (the PlatformIO trademark itself is not granted).