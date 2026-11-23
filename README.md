<p align="center"><img src="assets/logo.svg" width="96" alt=""></p>

<h1 align="center">sidenote</h1>

<p align="center">Scratch notes tied to the repo you're in, stored outside it, like Claude Code sessions.<br>
Made to sit in a side pane next to your agents, and to let them edit the note with you.</p>

## Install

```sh
curl -fsSL https://gbsn.dev/sidenote | sh
```

Installs a prebuilt binary for Linux or macOS (x86_64 / arm64) into `~/.local/bin`
(set `SIDENOTE_INSTALL_DIR` to change it). The URL redirects to the installer attached to the
[latest release](https://github.com/gaspb/sidenote/releases/latest/download/sidenote-installer.sh).
With Rust: `cargo install sidenote`, or `cargo binstall sidenote`.

Tip: `alias notes=sidenote`.

## Usage

```
sidenote               new note (TUI, autosaves)
sidenote -c            continue the most recent note in this repo
sidenote -r            pick a note (type to filter)
sidenote -r k3x        open a note by id prefix
sidenote ls [-a]       list notes for this repo (or all repos); ● = open in an editor
sidenote ls --json     same, machine-readable
sidenote show [<id>]   print a note (default: the one open in an editor, else the latest)
sidenote path [<id>]   a note's file path (same default); --dir for the repo's notes directory
sidenote rm <id>       delete a note
sidenote check <id> <line>  tick / untick the checkbox on that line (--expect <text>: find it by its text if the note moved)
sidenote setup herdr   give sidenote its name and icon in the herdr app
echo hi | sidenote     capture stdin as a new note, no TUI
```

The first line of a note is its title. Notes save as you type; empty ones are discarded on quit.
The terminal title (and the herdr pane title, where herdr allows it) follows the note.

## Editor

| Keys | |
| --- | --- |
| `^O` · `^T` · `^S` · `^Q` / `Esc` | notes picker · new note · save now · quit |
| `^C` · `^X` · `^V` | copy · cut · paste, using the system clipboard |
| `^Z` · `^Y` / `^⇧Z` | undo · redo |
| `^A` / `^E` · `^K` · `^U` · `^W` | line start / end · delete to end · delete to start · delete word (as in Claude Code) |
| `Enter` in a list | next item (same indent, next number, fresh `[ ]`); on an empty item, ends the list |
| `Tab` / `⇧Tab` | indent / outdent a list item |
| `^Space`, or click `[ ]` | tick / untick a checkbox |

Markdown is highlighted as you type: headings, **bold**, *italic*, `code`, code blocks, links, quotes,
lists and checkboxes (ticked items are dimmed). Your terminal's colours are kept.

Mouse: click to move the cursor, drag to select (copied to the clipboard), wheel to scroll.
Hold Shift to use your terminal's own selection instead.

## Working with an agent

Notes are plain markdown files, so agents edit them with their normal file tools. While the note is open,
the editor picks up outside changes every second. With no unsaved typing it reloads; with unsaved typing
it does a 3-way merge. If you both changed the same lines, your text wins and theirs is saved as a separate
"Conflicting copy" note, so nothing is lost.

`sidenote --skill` prints a guide for agents (how to find the note, edit it safely, create one), and
`--help` points agents to it. For Claude Code, add this to `~/.claude/CLAUDE.md`:

```
- The user keeps per-repo scratch notes with the `sidenote` CLI. When they mention "my note",
  "the note" or their notes, run `sidenote --skill` first and follow it.
```

## herdr

In the [herdr](https://herdr.dev) app, run once:

```sh
sidenote setup herdr
```

It adds a `sidenote` entry (name, [icon](assets/herdr-icon.svg), tile colour) to `~/.config/herdr-app/programs.json`,
keeping the rest of that file, so sidenote panes show as Sidenote with their own icon. herdr applies it live.

## Storage

`~/.local/share/sidenote/projects/<encoded-repo-path>/<id>.md` (override with `$SIDENOTE_HOME`).
The project is the main checkout of the git repo, so linked worktrees share notes; outside git it's the cwd.

## Releasing

`cargo release patch --execute` (or `minor` / `major`). The pushed `vX.Y.Z` tag triggers CI, which builds
the Linux/macOS binaries and the installer (cargo-dist), creates the GitHub Release and publishes to crates.io.

## License

MIT
