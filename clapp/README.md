# GridCraft as a clapp

GridCraft, built from source, with one addition: a CLI an agent drives the live window
through. The person works in GridCraft as always; the agent works on the same workbook with
`gridcraft <group> …`; both land on GridCraft's one undo history. `gridcraft help` is the
agent's manual and the only one.

## How it fits together

```text
Clatch ─ launches ─▶ bin/gridcraft app ─ starts ─▶ GridCraft.app --control <port>
agent  ─ gridcraft <group> … ─▶ ~/.gridcraft/gridcraft.sock ─▶ bin/gridcraft app
                                 ─ JSON lines, with the window's key ─▶ GridCraft
```

GridCraft already has a control port, a loopback JSON-lines protocol its own MCP bridge drives
(`docs/control-protocol.md`). **Upstream's took any request from anyone who could reach
127.0.0.1**, and a web page can (a `fetch` POST whose body is one JSON line). This fork ports
DeckCraft's answer to it (WordCraft's before that): a random key the window writes owner-only
once the port is bound, checked on every request, and a connection closed at the first line
that is not JSON. The launcher reads that key; nothing else of the app's is changed for the
clapp but `app.close`.

- **`clapp/`**: the Rust binary (`gridcraft`), one binary with two roles on
  [clappkit](../clappkit). `app` is the launcher Clatch starts; anything else is the agent's CLI.
  - `craft.rs` is everything that is GridCraft's own. The DeckCraft, PhotoCraft and WordCraft
    clapps hold the same file with their app's answers; `app.rs`, `cli.rs`, `policy.rs` and
    `out.rs` are the same in all of them, byte for byte.
- **`clatch.json`**: the manifest, with the six groups an agent may be granted.

## The groups are the grants

| group | reaches |
|---|---|
| `doc` | reads the open workbook, takes a picture of a sheet - changes nothing |
| `window` | reads the window's state, takes a picture of it - changes nothing |
| `command` | lists GridCraft's commands and what each takes - changes nothing |
| `run` | runs one command on the open workbook; never files or the app's preferences |
| `file` | new, open, save, export, close, insert pictures - the only group that touches the disk |
| `ui` | clicks, keys, typing, cells, dialogs, as a person would |

`run` reads an allow-list of namespaces, so a namespace upstream adds tomorrow stays out until
someone has read what is in it. Inside it, the person's own settings stay theirs (`view.theme`,
`view.formulaBar`, the spelling dictionary, sheet and workbook protection). `automate.*` is
never sent: a saved script runs commands past these rules. A command that names a file on disk
(`insert.picture {path}`, `data.getData {path}`) is refused by `run`; the same command with the
file's contents (`base64`, `text`) is not.

## What was measured, and the rule it became

These were found by driving the running app, not by reading its docs:

- **Closing the window throws away unsaved changes without asking**: its close button, Quit,
  and `app.quit` alike. So `app.quit` is never forwarded, and the patch's `app.close` asks
  instead: while a workbook has unsaved changes (or a cell is being edited) it brings up the
  app's own Save Changes question for it and leaves the window open; with nothing unsaved it
  closes.
- **The engine's `file.close` asks with that question, unless `force` is set, which throws the
  changes away.** **`ui.menu.invoke` and `command` are the engine's path here, not the menu's.**
  No `file.*`, `app.*` or `automate.*` command, no `ui.menu.invoke` and no `ui.printSubmit` is
  ever forwarded, by anyone (`policy::forward`, in the launcher), and `file close` refuses while
  the workbook has unsaved changes.
- **A save with no path on a workbook without a file opens GridCraft's Save As dialog and
  answers as if it had saved.** `file save` refuses that case before sending anything.
- **Only .xlsx (and .json) become the workbook's file**; a save to .csv, .tsv, .html or .pdf
  writes a copy and leaves the workbook unsaved. So `file save` takes .xlsx or .json, and
  `file export` writes the copies.
- **Switching sheets changes neither the unsaved state nor the undo history.** GridCraft has
  no off-screen picture of a sheet, so `doc render --page <n>` shows sheet n in the window,
  takes its picture, and puts the sheet in front back.
- Several workbooks can be open: `file new` and `file open` add one and replace nothing (an
  untouched blank Book1 is replaced, as File > Open does).

## Files between the window and the agent

Pictures do not travel in answers: the window writes them into `exchange/` in the clapp's data
folder and the CLI moves them to the agent's `--out` with the agent's own permissions. The other
way, `file insert` reads the agent's file in the CLI, puts a copy in `exchange/`, and the window
inserts that copy: the window never reads a path an agent names. Nothing lives in `exchange/` past
one request. When Clatch kills the launcher, its own cleanup does not run: the next start
empties `exchange/` before anything else.

## Upstream patches

Everything new is in `clapp/`, `clatch.json`, `assets/icon.png` and `clappkit`; upstream files
carry only these, each marked `clapp:`.

| file | change |
|---|---|
| `apps/gridcraft/src/control_server.rs` | DeckCraft's keyed server, renamed, with its tests (GridCraft's 64 MiB line limit kept) |
| `apps/gridcraft/src/main.rs` | start it with the settings folder; remove the key on exit |
| `crates/control-key/` | where the key file lives, shared by the app and the MCP bridge (new crate) |
| `crates/mcp/src/backend.rs`, `crates/mcp/src/tests.rs` | `gridcraft-cli mcp --connect` and `send` send the key, to loopback only |
| `Cargo.toml`, `apps/gridcraft/Cargo.toml`, `crates/mcp/Cargo.toml` | the crate, and `getrandom` |
| `xtask/src/layers.rs` | `control-key` registered at L0 |
| `crates/ui-egui/src/control.rs` | `app.close`: close the window without losing unsaved work |
| `docs/control-protocol.md` | Keys and `app.close` |
| `AGENTS.md` | a pointer to this file at the top |
| `ATTRIBUTION.md` | the row for `assets/icon.png` |
| `.gitignore` | `pkg/` and `*.clapp` |
| `.github/workflows/` | removed: upstream's CI and release jobs would run on Arfium's Actions minutes |

`assets/icon.png` is GridCraft's own mark, regenerated by `clapp/scripts/icon.sh` from
`assets/app-icon/gridcraft-1024.png`: 1024 x 1024, the tile full-bleed, as
`clappkit/docs/icons.md` asks.

## Build and run (macOS, development)

```bash
CARGO_TARGET_DIR=~/.cache/gridcraft-clapp/target cargo build --release -p gridcraft
CARGO_TARGET_DIR=~/.cache/gridcraft-clapp/clapp-target cargo build --manifest-path clapp/Cargo.toml
CLATCH_STANDALONE=1 GRIDCRAFT_CLAPP_GUI=~/.cache/gridcraft-clapp/target/release/gridcraft \
  ~/.cache/gridcraft-clapp/clapp-target/debug/gridcraft app &
~/.cache/gridcraft-clapp/clapp-target/debug/gridcraft help
```

`cargo test --manifest-path clapp/Cargo.toml` holds the rules above and checks that
`clatch.json` declares exactly the groups the help gives. The fork has no CI.

Based on GridCraft's `main` at workspace version 0.6.1 (remote `upstream`).

## Not done yet

- **Stopping.** When Clatch stops the launcher, the window it started stays open.
- **The depot.** Nothing packages the clapp for `clatch install` yet.
- **Signals.** Nothing yet tells the agent what the person did.
- **Windows.** `craft::BUNDLE_EXE` names only the macOS bundle's window.
