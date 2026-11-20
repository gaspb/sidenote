# sidenote: guide for agents

The user keeps scratch notes per repository with `sidenote` (they may type it as `notes`, an alias you
can't rely on, so always call `sidenote`). Notes are plain markdown files stored outside the repo. Each note
has a short id like `k3x9qa`, and its first line is its title. The repo is detected from the current
directory, so run these commands from inside the repo.

## Find the note

    sidenote path            path of the note the user is working on (open in their editor, else the latest)
    sidenote show [ID]       print it (same default)
    sidenote ls --json       every note for this repo: id, title, path, modified, open
    sidenote path ID         path of a specific note (an id prefix is enough)

If the user says "my note" or "the note" without saying which, use `sidenote path`.

## Edit it

- Edit the file in place with small, targeted edits (your Edit / str_replace tool). Change only what was asked.
- Don't rewrite or reformat the whole file. The user may be typing in it right now: their editor merges
  outside edits line by line, so small edits merge cleanly and whole-file rewrites conflict.
- Keep the first line (the title) unless asked to change it.
- Leave other files in the notes directory alone (`*.lock` files mark notes open in an editor).

## Create a note

    printf '%s\n' "# Title" "body" | sidenote     creates a note for this repo and prints its id

## Don't

- Don't run `sidenote`, `sidenote -c` or `sidenote -r` without piping stdin: they open a full-screen editor.
- Don't delete notes (`sidenote rm`) unless asked.
- Notes live outside the repo on purpose. Don't copy them into the repo or commit them.
