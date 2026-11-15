# PlatformIO for OpenIDEA: Core setup and the project loop

Date: 2026-11-15
Status: implemented (0.2.0) — build/upload/monitor/check/clean/add-board/detect-board via keyed terminals; Run-configurations path deferred (see decisions)
Scope: pieces 1 and 2 of 6 (see "Later pieces")

## Intent

PlatformIO's IDE integration only exists for VS Code. This plugin brings it to
OpenIDEA so that embedded work never needs VS Code. The long-term aim is that
everything PlatformIO Core can do is reachable from inside OpenIDEA.

This document covers the first release: getting Core onto the machine and
approved (piece 1), and the daily edit, build, upload, monitor loop (piece 2).
Everything else in Core is reachable in this release by running any `pio`
command from the IDE, and gets proper UI in later pieces.

The reference project is `~/Documents/PlatformIO/Projects/c3bridge`: one
environment (`esp32-c3-devkitm-1`), Arduino framework, monitor at 115200.

## Goals

1. A user with no PlatformIO installed can go from installing the plugin to
   uploading firmware without leaving OpenIDEA or opening VS Code.
2. A user who already has Core (for example from the VS Code extension, at
   `~/.platformio/penv/bin/pio`) is detected and reuses it.
3. Build, Upload, Clean, Upload and Monitor, and Serial Monitor are each one
   click or one shortcut away, for the active environment.
4. Build, Upload, Monitor, Check and Clean run in keyed terminals (PTY, so the
   serial monitor works): one tab per operation, reused while running. Run
   configurations were considered but deferred: the host only refreshes a
   plugin's exported configs when the Run list is built, and dynamic configs
   are filtered unless `process:pio` is pinned, so a plugin-initiated first
   Build could fail with "no owned run configuration with this id".
5. Switching environment is one click and is remembered per project.
6. clangd understands the project (go to definition, completion, errors) with
   no manual configuration beyond what this document names.
7. Any `pio` command can be run from the IDE, so all 18 Core commands are
   usable from day one.
8. A new PlatformIO project can be created from inside the IDE.

## Non-goals (this release)

- Library and platform management UI (`pkg`, platform install). Piece 3.
- Boards: add-board and detect-board are implemented (append an env via
  `pio project init -b <board>`); browsing all 1600+ boards lands in piece 3.
- Unit tests in the test tree, static analysis in Problems (`test`, `check`). Piece 4.
- Debugging (`debug`). Piece 5.
- Account, organisations, teams, remote, CI, PIO Home. Piece 6.
- `.ino` sketches and the Arduino language server.
- Remote (SSH) projects. Not blocked on purpose, but not tested or supported.
- Windows and Linux testing. Nothing may be written in a way that only works on
  macOS, but only macOS is verified in this release.
- Turning compiler output into Problems entries. The Run console is enough for now.

## Anti-goals

- Bundling or reimplementing Core. The plugin drives the real `pio`.
- Running anything through a shell. Every process is `pio` or `python3` with an
  argument list.
- Starting processes the user did not ask for. Nothing runs on project open
  except reading `platformio.ini`.
- Editing the user's `platformio.ini`, `.gitignore` or other project files.
- A UI that only makes sense to someone who knows the VS Code extension.

## Constraints found in the plugin API

These come from the SDK and docs at tag `v0.1.45` (plugin API 1.8, which is
also what the installed OpenIDEA 0.1.45 implements). The checkout is at
`~/.cargo/git/checkouts/openidea-7faf5f9c9e6a9886/951e81e`; `docs/en/plugins/`
and `plugins/README.md` there are the references. Note that `~/dev/openidea`
is an older fork whose plugin API is 0.1 and must not be used as a reference.

- The plugin is a sandboxed WebAssembly component. It has no file, network or
  process access except through host functions gated by capabilities.
- `process:<program>` runs only the exact executable the user approved, pinned
  by absolute path and content hash. At grant time the IDE looks the name up on
  `PATH`. If it is not there, the capability is granted with no executable, runs
  fail, and the user has to choose the executable in Settings › Plugins. The
  plugin cannot choose it for them.
- `pio` is commonly not on `PATH` (it is not on this machine). So the one-time
  manual approval is part of the normal first-run path, not an edge case.
- All calls are synchronous. A command or event handler has 5 seconds, or 60
  seconds renewed by progress reports while a task is open. An idle plugin
  instance is dropped after 5 minutes and loses its in-memory state.
- Run configurations returned by the plugin and terminal sessions it opens are
  owned by the IDE and survive the plugin being idle. Their exits come back as
  `run-exited` and `terminal-exited` events.
- The run-configurations export is called without a window and cannot prompt.
- The export is **cached**. The IDE re-asks the plugin only on project open, on
  Run › Reload Run Configurations, on some settings changes, or when a file
  changes that matches a fixed list (`CONFIG_FILES` in `src/features/run/index.ts`)
  which does not contain `platformio.ini`. `host::start_run` resolves against
  that same cache, so starting a task for an environment the cache does not know
  fails with "no owned run configuration".
- **Every exported configuration appears in the Run dropdown**, so a
  configuration that should stay out of the dropdown cannot be exported.
- `host::start_run(config, args, debug)` **appends** `args` to the exported
  configuration's own arguments at launch time. Anything that varies (today the
  port) therefore need not be frozen into the export.
- **The plugin cannot refresh the export itself.** Calling the IDE action
  `run.reloadConfigs` through `ide.commands` re-enters the plugin's own export
  while the plugin is blocked waiting for the answer: the frontend awaits the
  action (`src/features/plugins/runtimeUi.tsx`, `answerCommand`), the action
  lists configurations, and listing calls `refresh_run_configurations`, which
  calls back into the busy instance. That call is given 50 ms before the host
  counts it as stalled and drops the instance, so the export fails and the
  command dies with the instance it was running in. A dropdown that follows
  `platformio.ini` by itself is therefore not available, and `ide.commands` is
  not used for it.
- Runs started from the Run dropdown or by Rerun produce no event for the
  plugin: the operation id is only valid for the run the plugin itself started.
- Declared status bar items have no `when` clause, so showing them only in
  PlatformIO projects has to be done from code, per window.
- A plugin terminal runs one approved program. It cannot open a shell.

## Approaches

Three ways to run build and upload were considered.

**A. Through the IDE's Run system (chosen).** Each environment's tasks become
run configurations. Output goes to the Run tool window. The serial monitor uses
a terminal tab because it needs keyboard input. Gains: Run button, Stop, Rerun,
Run dropdown entries, and a reliable `run-exited` event to chain "upload, then
monitor". Costs: more to build than B.

**B. Everything in terminal tabs.** Every task runs `pio` in a terminal, as the
VS Code extension does. Gains: least work, uniform behaviour. Costs: no Run
button, Stop or Rerun, and OpenIDEA's run features go unused. It would be VS
Code's experience in a different window.

**C. The plugin runs `pio` itself and streams output (dropped).** Gives the most
control, but call time limits and the 5-minute idle reclaim would kill a long
first ESP32 build, and at most 8 child processes are allowed.

Darwin chose A, on the recommendation that it is what makes the plugin feel
native to OpenIDEA.

## What the user sees

Darwin delegated these decisions ("you decide what is best"). They are the
designer's calls, not requirements he stated.

### Status bar

Shown only in windows whose project has a `platformio.ini` at its root:

- Environment: `env: esp32-c3-devkitm-1`. Click opens a picker of environments.
- Build, Upload, Monitor: three items, each running that task for the active
  environment.
- Port: `port: auto` or the chosen port. Click opens the device picker.

### PlatformIO tool window

Always in the stripe: no `when` clause, because Darwin works in projects that
have no `platformio.ini` and still expects the button to be there. In the
**bottom dock**: `location = "bottom"`, which OpenIDEA calls "Bottom Left" — the
button sits at the bottom left of the window, the leftmost of the bottom bar,
and the panel opens across the window's width. That width is the point: build
and monitor output is horizontal, and the left dock is a narrow column however
it is split (Darwin asked for the bottom left for exactly this). The plugin logo
is its button. A placement the user drags wins over this, as in any tool window.

The window lists **every tool the plugin has**, grouped, so it is both the map
of the plugin and the way to run things:

```
Core
  Install PlatformIO Core          downloads and runs get-platformio.py
  Upgrade PlatformIO Core          pio upgrade
  Show Core Version                pio --version
Tasks
  Build                            pio run -e <env>
  Upload                           pio run -e <env> -t upload
  Upload and Monitor               upload, then the monitor if it passed
  Monitor                          pio device monitor -e <env>
  Clean                            pio run -e <env> -t clean
  Full Clean                       pio run -e <env> -t fullclean
  Run Target…                      pio run -e <env> -t <target>
  Update Code Index                pio run -e <env> -t compiledb
Project
  Select Environment               the [env:…] sections of platformio.ini
  Select Port                      pio device list
  New Project…                     pio project init --sample-code
  Run PlatformIO Command…          any pio command
```

The grey text after each row is the Core work behind it: it makes the window
readable before anything is connected, and it is the tooltip for the row. The
Tasks rows act on the active environment, which the status line and the status
bar show; environments are not repeated as a tree here, because Select
Environment is one click and a project with eight environments would otherwise
fill the window with duplicates. Double click or Enter runs the row.

The status line states the project's standing: `No project is open`, `No
platformio.ini at the project root`, or the active environment once the plugin
knows it. In a project that is not a PlatformIO project the window still lists
the tools, because that is how a user finds out what the plugin does and that
the project needs a `platformio.ini`. When Core is not ready the body leads with
a message that says what to do next, with a button for it.

### Logo

The plugin logo shows in three places:

- the PlatformIO tool window's button on the bottom left tool window bar (the
  view's `icon`), which is the placement Darwin asked for;
- Settings › Plugins (the manifest's `icon`, with `icon-dark` if the mark needs
  a lighter variant on dark themes);
- the Project tree, as the file icon of `platformio.ini`
  (`[[contributes.fileIcons]]`).

The status bar cannot show it: plugin status bar items are text only.

Artwork (designer's call, Darwin has not chosen): the official PlatformIO mark,
taken from the VS Code extension already on this machine
(`~/.vscode/extensions/platformio.platformio-ide-*/assets/images/platformio-mini-logo.svg`,
2.5 KiB, no scripts or external references, so it fits the 16 KiB sanitised-SVG
limit). It is copied into the package as `icons/platformio.svg`. That extension
is Apache-2.0, which allows copying the file with attribution but grants no
right to the PlatformIO trademark. This is acceptable for a plugin used
locally. Before the plugin is published anywhere, either PlatformIO's brand
terms are checked or the file is swapped for an original mark; the README says
so. The tool window button is small and single-colour in most themes, so the
builder checks the mark is legible there in both light and dark themes and
adds a dark variant if it is not.

### Commands (Find Action, bindable)

All under the category "PlatformIO", acting on the active environment:

Build, Upload, Upload and Monitor, Monitor, Clean, Full Clean, Run Target…,
Select Environment, Select Port, Update Code Index, New Project…, Run
PlatformIO Command…, Install PlatformIO Core, Upgrade PlatformIO Core, Show
PlatformIO Core Version.

No default keyboard shortcuts are declared in this release.

### Run dropdown

For every environment: `PlatformIO: Build (<env>)`, `PlatformIO: Upload (<env>)`
and `PlatformIO: Clean (<env>)`. No other task is exported, because everything
exported lands in the dropdown and it should stay short in projects with many
environments.

The exported arguments hold nothing that changes while the project is open: no
port, no target. Switching the active environment, or picking a different port,
therefore never needs the export to be refreshed, and the plugin's own tasks
append what they need at launch.

What the cache costs, and how it is handled:

- Adding, removing or renaming an environment changes the set of
  configurations. The tree and status bar follow at once; the dropdown follows
  on the next reload. When the plugin sees that the environment set changed, it
  says so once, in a notification that names Run › Reload Run Configurations.
  Until that reload, starting a task for a new environment is refused with the
  same instruction rather than guessed at: `-e` is repeatable in Core
  (`pio run -e a -e b` processes both), so appending the new environment to an
  exported configuration would run two, and that is exactly how a wrong guess
  would flash the wrong board.
- An upload started from the dropdown or by Rerun is the plain one: no
  `--upload-port`, and the plugin cannot stop or reopen the monitor for it,
  because it is not told the run began. Upload from the status bar, the tree or
  Find Action is the one that applies the port and handles the monitor. The
  README and the tool window say so.

## Behaviour

### Environments

- The plugin reads `platformio.ini` itself and takes environment names from
  `[env:NAME]` sections, following `extra_configs` in `[platformio]`. It does
  not call `pio` for this, because the run-configurations export has no window
  and must be fast.
- The active environment is stored per project. Its initial value is the first
  entry of `default_envs` if set, otherwise the first environment in the file.
- Every task passes `-e <env>` explicitly.
- When `platformio.ini` changes, the tree and the status bar follow at once. The
  run configurations follow on the next reload (see Run dropdown). If the active
  environment no longer exists, the initial-value rule applies again.

### Tasks

| Task | Runs | Where |
|---|---|---|
| Build | `pio run -e E` | Run tool window |
| Upload | `pio run -e E -t upload` | Run tool window |
| Clean | `pio run -e E -t clean` | Run tool window |
| Full Clean | `pio run -e E -t fullclean` | Terminal tab |
| Run Target… | `pio run -e E -t <target>` | Terminal tab |
| Update Code Index | `pio run -e E -t compiledb` | Terminal tab |
| Monitor | `pio device monitor -e E` | Terminal tab |
| Upload and Monitor | Upload, then Monitor if it exited 0 | Both |

- Build, Upload and Clean are the only tasks exported as run configurations, so
  they are the only ones in the dropdown and the only ones with Stop and Rerun.
  They are also the three the loop repeats. Full Clean, Run Target… and Update
  Code Index are one-shot: their output is read once, their tab title is always
  truthful, and exporting them would add three more entries per environment to
  the dropdown.
- When a port is chosen, the plugin appends `--upload-port <port>` when it
  starts Upload, and Monitor adds `--port <port>`. With `auto`, nothing is
  added and Core detects the port. The port is never part of the export.
- Run Target… offers the targets from `pio run -e E --list-targets` in a picker,
  with a way to type a target name that is not listed. This is how
  platform-specific targets such as Upload Filesystem Image and Erase Flash are
  reached in this release.
- Select Port lists `pio device list --json-output` plus `auto`. The choice is
  stored per project.

### Monitor and upload sharing the port

There is one monitor terminal per project window. If it is running when an
upload the plugin starts begins, the plugin stops it first (Ctrl+C to the
terminal), runs the upload, and reopens the monitor when the upload exits 0. If
the upload fails, the monitor stays closed and the Run tool window shows why.
An upload started from the dropdown or by Rerun does not do this, and usually
fails to open the port the monitor holds; the README says to use the plugin's
own Upload for the full loop.

### Code intelligence

OpenIDEA's built-in C++ support starts clangd when `compile_commands.json` is
at the project root. `pio run -t compiledb` writes that file.

- The index is out of date when `compile_commands.json` is missing, is older
  than `platformio.ini`, or was generated for a different environment than the
  active one.
- When it is out of date, the plugin shows one notification per project
  session with an **Update** button. It does not start the process by itself.
  A setting, `platformio.promptCodeIndex` (default true), turns the prompt off.
- Update Code Index is also a command and a toolbar button.

Known risk: for cross toolchains such as the ESP32's, clangd may not find the
toolchain's own headers unless it is started with `--query-driver`. OpenIDEA
lets the user change clangd's arguments in Settings › Languages & Frameworks ›
Language Servers, but a plugin cannot. This must be tried on `c3bridge` early.
If it is needed, the plugin detects the toolchain path and tells the user the
exact argument to add, and the README documents it.

### Core setup

Run on the first PlatformIO action in a session, and by Install PlatformIO Core.

1. Run `pio --version`. If it works, Core is ready; remember the version.
2. If it fails because no executable is approved, look for an existing install
   at `~/.platformio/penv/bin/pio` (`%USERPROFILE%\.platformio\penv\Scripts\pio.exe`
   on Windows). If found, tell the user Core was found at that path and that
   they need to select it for `process:pio` in Settings › Plugins. Show the full
   path so it can be copied.
3. If none is found, offer to install. On yes: download PlatformIO's official
   installer script (`get-platformio.py` from the `platformio-core-installer`
   repository), save it in the plugin's private directory, and run it with
   `python3` in a terminal tab so progress is visible. When it exits 0, go to
   step 2. If `python3` is missing or not approved, say so and link to
   PlatformIO's install page.
4. Upgrade PlatformIO Core runs `pio upgrade` in a terminal tab. If the IDE then
   refuses `pio` because the file changed, show the step 2 guidance again.

Until Core is ready, tasks do not fail with a raw error. They show the same
guidance as the tool window.

### Running any Core command

Run PlatformIO Command… asks for arguments (for example `pkg install -l
bblanchon/ArduinoJson`), splits them the way a shell would (quotes respected, no
expansion, no shell), and runs `pio <args>` in a terminal tab in the project
root. The last 20 inputs are remembered per project and offered again.

This replaces the "terminal tab with `pio` ready" that was proposed in
conversation: a plugin terminal can only run an approved program, not a shell.
The README tells users who want `pio` in OpenIDEA's own terminal to add
`~/.platformio/penv/bin` to their `PATH`.

### New project

New Project… asks, in order: parent folder, project name, board (picker over
`pio boards --json-output`, searchable by name and id), framework (the board's
frameworks; skipped when there is one). It then runs, in a terminal tab:

`pio project init -d <parent>/<name> -b <board> -O framework=<fw> --sample-code`

and opens the new folder as a project when it exits 0. `--sample-code` makes
Core write a starter `main` file so the project builds immediately.

## Manifest and capabilities

- `id` stays `openidea-platformio`; `name` becomes `PlatformIO`; categories
  `tools`, `frameworks`; `icon = "icons/platformio.svg"`.
- `engines.openidea` is the lowest 1.x that `openidea-plugin check` accepts for
  the features used. Run configurations, keyed terminals, window status items
  and owned exit events need at least `^1.4`.
- Required: `project.read`, `process:pio`, `events`, `storage`.
- Optional: `process:python3` and `net:raw.githubusercontent.com` (install
  only), `fs.read:~/.platformio` (detect an existing install).
- `ide.commands` is **not** requested: the one thing it was wanted for, making
  the Run dropdown follow `platformio.ini`, does not work from inside the plugin
  (see the constraints above), and nothing else needs it.
- The plugin must work with every optional capability refused: install then
  explains what is missing, and detection falls back to asking the user to
  select the executable.

## Technical shape

- Rust, on the existing scaffold, SDK pinned to tag `v0.1.45`. The two sample
  commands and their tests are removed.
- Pure logic is kept apart from host calls so it can be unit tested natively:
  `platformio.ini` parsing (sections, `default_envs`, `extra_configs`, comments,
  multi-line values), argument splitting, building each task's argument list,
  the out-of-date rule for the code index, parsing `--list-targets` and the
  JSON outputs.
- State that must survive idle reclaim (active environment, chosen port, command
  history, a pending "monitor after upload") lives in `host::storage`, keyed by
  project root.
- Tests use the SDK's mock host (`cargo test`). `openidea-plugin build` must end
  with `ok`.

## Done means

On this machine, with the plugin linked into OpenIDEA and `c3bridge` open:

1. The first-run guidance leads to approving `~/.platformio/penv/bin/pio`, and
   after that Show PlatformIO Core Version reports 6.2.0.
2. Build from the status bar compiles `c3bridge` in the Run tool window, and
   Stop and Rerun work.
3. Upload flashes the ESP32-C3, and Upload and Monitor opens the monitor at
   115200 afterwards.
4. Starting an upload from the status bar while the monitor is open closes the
   monitor, uploads, and reopens it.
5. After Update Code Index, go to definition on an Arduino symbol in
   `src/main.cpp` works (with the `--query-driver` step if it proved necessary).
6. Run PlatformIO Command… with `device list` prints the ports.
7. New Project… creates a project for `esp32-c3-devkitm-1` that builds.
8. With a second environment added to `platformio.ini`, the switcher and tree
   show it at once, the plugin says a reload is needed, and after Run › Reload
   Run Configurations the dropdown lists it and tasks run for it.
9. The PlatformIO logo is the tool window's button on the bottom left tool
   window bar, the window lists every tool grouped as above, and the logo is
   legible in a light and a dark theme.
10. `cargo test` passes and `openidea-plugin build` ends with `ok`.

Items 3 and 4 need the board plugged in and are checked by Darwin.

## Left to the builder

- Module layout and naming inside `src/`.
- How status bar items are attached to the right windows, given that only
  commands and events carry a window. If they cannot appear before the first
  window event (a file opened or focused), that delay is acceptable.
- Which event subscriptions and activation entries keep the tree and status bar
  in step with `platformio.ini`, and how the plugin notices that the environment
  set changed so it can raise the reload notification once.
- How the plugin recognises that the cached export is out of date: it cannot see
  the export, so it compares the environments in `platformio.ini` with the ones
  it last exported in this session, kept in `host::storage`.
- Exact wording of notifications, messages and tooltips, in plain English.
- Icons for tree items and toolbar buttons, from OpenIDEA's built-in icon names
  (the logo itself is decided above).
- Whether New Project needs an open trusted project to start a terminal. If it
  does, say so clearly in the error.

## Later pieces

In the agreed order, each with its own design: (3) libraries, platforms and
boards; (4) tests and static analysis; (5) debugging; (6) account and cloud.
