# sidenote

Scratch notes tied to the repo you're in, stored outside it (like Claude Code sessions).
Made to sit in a side pane next to your agents, and to let them edit the note with you.

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
echo hi | sidenote     capture stdin as a new note, no TUI
```

Tip: `alias notes=sidenote`.

Editor keys: `^O` notes picker · `^T` new note · `^S` save now · `^Q`/`Esc` quit · `^Z`/`^U` undo · `^R` redo.
The first line is the title. Empty notes are discarded on quit.

## Working with an agent

Notes are plain markdown files, so agents edit them with their normal file tools. While the note is open,
the editor picks up outside changes every second. With no unsaved typing it reloads; with unsaved typing
it does a 3-way merge. If you both changed the same lines, your text wins and theirs is saved as a separate
"Conflicting copy" note, so nothing is lost.

`sidenote --skill` prints a guide for agents (how to find the note, edit it safely, create one).
`--help` points agents to it. For Claude Code, add this to `~/.claude/CLAUDE.md`:

```
- The user keeps per-repo scratch notes with the `sidenote` CLI. When they mention "my note",
  "the note" or their notes, run `sidenote --skill` first and follow it.
```

## Storage

`~/.local/share/notes/projects/<encoded-repo-path>/<id>.md` (override with `$NOTES_HOME`).
The project is the main checkout of the git repo, so linked worktrees share notes; outside git it's the cwd.

## Install

`cargo install --path .`
