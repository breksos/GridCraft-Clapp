# GridCraft as a clapp

This repository is [GridCraft](https://github.com/storytold/gridcraft) (remote `upstream`), made
into a Clatch app on [clappkit](clappkit/README.md), the way
[template-clapp](https://github.com/arfium/template-clapp) builds one. Upstream's own docs and
rules (`AGENTS.md`, `docs/`) still apply; this page is only what the fork adds.

## How it fits together

One binary, two roles, one state (clappkit `docs/architecture.md`):

```text
person ─ clicks, keys ──▶ GridCraft window ─┐
                                            ├─▶ Session (the workbook) ─ one undo history
agent ── gridcraft <verb> ─▶ ~/.gridcraft/gridcraft.sock ─┘
Clatch ◀── control pipe: register, signals ── the window
```

- `gridcraft app` is the window Clatch launches. It runs only under Clatch: started any other
  way it asks Clatch to start it (`CLATCH_STANDALONE=1` runs it without, for development).
- `gridcraft <verb>` is the agent's CLI. A request arrives on clappkit's socket and becomes a
  request on the queue GridCraft's own control channel feeds, so it runs through the same
  `SheetApp::run` the ribbon does, and the window shows it as it happens.
- Under Clatch, settings and logs live in `CLATCH_DATA_DIR`, which uninstalling can erase.

All of it is in [`apps/gridcraft/src/clapp/`](apps/gridcraft/src/clapp/mod.rs):

| file | is |
|---|---|
| `mod.rs` | which role a launch is; the bootstrap |
| `cli.rs` | the verbs and `gridcraft help`, the agent's only manual |
| `door.rs` | the window's side of the socket: each verb as GridCraft control methods |
| `rules.rs` | what an agent never does here |
| `signals.rs` | what the person does, told to the agent |
| `recovery.rs` | unsaved work kept on disk and reopened after the window ends |
| `wiring.rs` | the control pipe and the socket, on a runtime beside eframe |

## The verbs are the grants

Each is declared in `clatch.json` and is its own permission (`Bash(gridcraft <verb>:*)`).

| look | change the workbook | files | the window |
|---|---|---|---|
| `state` `read` `commands` `shot` | `set` `write` `run` | `new` `open` `save` `export` `insert` | `focus` `close` |

## Signals: the agent keeps up with the person

| signal | type | carries |
|---|---|---|
| `changed` | context | the commands the person ran (and their selection), once they pause for 1.2 s, one notice per burst |
| `selected` | buffered | the workbook, sheet, selection and active cell (value and formula), latest only, riding the person's next message |

Only the person's actions signal; the agent already knows its own. Within a frame the agent's
requests run in `SheetApp::logic` (its control poll) and the person's input in `ui`, so the app
observes before `logic` and settles after it: GridCraft's command journal and the selection say
what changed in between.

## Unsaved work is never lost

Under Clatch the window ends whenever Clatch stops it or its pipe drops, and GridCraft quits
without asking, so unsaved changes used to be gone. [`recovery.rs`](apps/gridcraft/src/clapp/recovery.rs)
keeps a copy of every workbook with unsaved changes in `<data dir>/recovery/`, written on a
worker thread within about a second of the last change (every 5 s at most during a long burst).
Nothing relies on a shutdown hook, which Clatch leaves no time for.

- The next launch reopens each copy as "<title> (recovered)": unsaved, with no file, so saving
  asks where and never overwrites the original unasked. GridCraft's own message box says what
  was recovered and from which file.
- A copy is removed when its workbook is saved, undone back to its saved state, or closed in
  the app ("Don't Save" included). Quitting the window keeps them: that is the case they are for.
- Covers a crash too. A copy that cannot be opened is left in place and named in the message.

Measured: changes by the agent and by the person, the pipe cut, relaunch: both came back with
their formulas, unsaved, and the copy was kept again; saving and "Don't Save" removed it.

## Updates come from Clatch

GridCraft can update itself (`craft_update`: a startup check, "Update and Restart", Help ▸ Check
for Updates…). Under Clatch that would rewrite the installed depot behind Clatch's back, so the
window never checks, offers or installs an update there; the menu item says to update in
Clatch. Outside Clatch, upstream's updater works as before.

## What was measured, and the rule it became

Found by driving the running app, not by reading its docs:

- **Closing the window throws away unsaved changes without asking** (the close button, Quit,
  `app.quit`). `gridcraft close` refuses while anything is unsaved or a cell is being edited.
- **`file.close {force: true}` throws one workbook's changes away.** `run` never sends `file.*`,
  `app.*`, `automate.*` (a script runs commands past these rules) or `ui.*`, nor the person's
  preferences (`view.theme`, `view.formulaBar`, the dictionary, protection).
- **New in 0.15:** `window.close {force}` (a workbook can have several windows) can throw
  unsaved work away, so it is refused; `formulas.pasteName` types into the cell the person is
  editing; `view.expandFormulaBar` and `view.statusBar` are preferences. The new namespaces stay
  out of `run`: `python` (code that runs), `macro` (steps past these rules), `link` (opens a
  browser), `options`, `app`, `automate`, `ui`. `equation` (editing an equation) is reached.
- **A save with no path on a workbook without a file opens the Save As dialog**: before 0.15 it
  answered as if it had saved; since, the native dialog blocks the whole window until someone
  answers it. `gridcraft save` refuses that case.
- **Only .xlsx (and .json) become the workbook's file**; .csv, .tsv, .html and .pdf are copies
  that leave it unsaved. `save` takes .xlsx/.json; `export` writes the copies.
- **Switching sheets changes neither the unsaved state nor the undo history.** GridCraft has no
  off-screen picture of a sheet, so `shot --sheet <n>` shows that sheet for the picture and puts
  the one in front back.
- **The control socket's directory may not be a symlink** (clappkit refuses `/tmp` on macOS).

## Upstream patches

Everything new is `apps/gridcraft/src/clapp/`, `clatch.json`, `CLAPP.md`, `bin/`, `scripts/`,
`package.json`, `.github/`, `.gitattributes`, `assets/icon*.png` and `clappkit`. Upstream files
carry only these, each marked `clapp:`:

| file | change |
|---|---|
| `apps/gridcraft/src/main.rs` | role dispatch and the bootstrap; the clapp hooks (signals, recovery copies) around `logic` and `ui`, as the last field of `App`; settings in `CLATCH_DATA_DIR`; no self-update under Clatch; no `windows_subsystem` (the CLI needs a console) |
| `crates/ui-egui/src/updates_ui.rs` | `updates_come_from`: under Clatch GridCraft never checks for, offers or installs an update itself; Help ▸ Check for Updates… says updates come from Clatch |
| `apps/gridcraft/Cargo.toml` | `clappkit` (core only) and `tokio` |
| `Cargo.toml` | `clappkit` excluded from the workspace (it has its own) |
| `AGENTS.md` | a pointer to this file |
| `ATTRIBUTION.md` | rows for `assets/icon.png` and `assets/icon-dock.png` |
| `.gitignore` | `pkg/` and `*.clapp` |
| `.github/` | upstream's workflows removed, including ones it adds later (FreeBSD, Nix, packaging lint, SonarCloud, Windows ARM64 so far), and its Dependabot configuration: they would run on this fork's Actions minutes or open pull requests here; template-clapp's CI (manual) and release (on a `v*` tag) in their place |

## Build, verify, ship

```sh
git clone --recurse-submodules https://github.com/breksos/GridCraft-Clapp.git
npm run verify          # build -> package -> clatch validate -> CLI <-> window round-trip
npm run pack            # pkg/ -> com.arfium.gridcraft-<os>-<arch>.clapp
CLATCH_STANDALONE=1 cargo run -p gridcraft -- app    # the window, without Clatch
```

`npm run check` checks that `clatch.json`, the CLI's verbs and help, and the emitted signals
agree. `cargo xtask ci` is upstream's own gate. `pkg/` and `*.clapp` are build output, never
committed. Push a `v*` tag and the release workflow publishes one `.clapp` per platform, so
`clatch install breksos/GridCraft-Clapp` works.

Based on GridCraft's `main` at workspace version 0.15.0 (upstream `ef927e2e`). Updating: merge
`upstream/main`, keep this fork's workflows, then re-check every command upstream added inside
the namespaces `run` reaches (`rules::NAMESPACES`) against the measurements above; a new
namespace stays out until someone has read it.

## Not done yet

- **Under a live Clatch.** Verified against a stand-in for the daemon's pipe; `clatch install`,
  `run` and `stop` still to be checked on a running daemon.
- **Windows** is built by the workflows but not yet run.
