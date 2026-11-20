# sidenote

Scratch notes tied to the repo you're in, stored outside it (like Claude Code sessions).

```
notes              new note (TUI, autosaves)
notes -c           continue the most recent note in this repo
notes -r           pick a note (type to filter)
notes -r k3x       open a note by id prefix
notes ls [-a]      list notes for this repo (or all repos); ● = open in another pane
notes show <id>    print a note (pipe it into claude, etc.)
notes rm <id>      delete a note
notes path [<id>]  storage dir for this repo (or a note's file)
echo hi | notes    capture stdin as a new note, no TUI
```

Editor keys: `^O` notes picker · `^T` new note · `^S` save now · `^Q`/`Esc` quit · `^Z`/`^U` undo · `^R` redo.
The first line is the title. Empty notes are discarded on quit.

Storage: `~/.local/share/notes/projects/<encoded-repo-path>/<id>.md` (override with `$NOTES_HOME`).
The project is the main checkout of the git repo, so linked worktrees share notes; outside git it's the cwd.

Install: `cargo install --path .` (installs the `notes` binary)
