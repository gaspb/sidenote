mod store;
mod tui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Read};
use store::Project;

const SKILL: &str = include_str!("skill.md");

/// Scratch notes tied to the current repo, stored outside it.
///
/// Run `sidenote` to start a new note, `sidenote -c` to continue the latest one,
/// or `sidenote -r` to pick from this repo's notes. Notes autosave as you type.
#[derive(Parser)]
#[command(
    name = "sidenote",
    version,
    after_help = "Are you an AI agent? Run `sidenote --skill` to learn how to read and edit the user's notes."
)]
struct Cli {
    /// Print the guide for AI agents working with these notes
    #[arg(long, exclusive = true)]
    skill: bool,

    /// Continue the most recent note in this repo
    #[arg(short = 'c', long = "continue", conflicts_with = "resume")]
    cont: bool,

    /// Open the note picker, or a note directly by id (prefix is enough)
    #[arg(short, long, value_name = "ID", num_args = 0..=1, default_missing_value = "")]
    resume: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List notes for this repo
    #[command(visible_alias = "ls")]
    List {
        /// List notes for every repo
        #[arg(short, long)]
        all: bool,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Print a note (default: the one open in an editor, else the latest)
    #[command(visible_alias = "cat")]
    Show { id: Option<String> },
    /// Delete a note
    Rm {
        id: String,
        /// Delete even if it is open in another pane
        #[arg(short, long)]
        force: bool,
    },
    /// Print a note's file path (default: the one open in an editor, else the latest)
    Path {
        id: Option<String>,
        /// Print this repo's notes directory instead
        #[arg(long, conflicts_with = "id")]
        dir: bool,
    },
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("sidenote: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.skill {
        print!("{SKILL}");
        return Ok(());
    }
    let project = Project::current()?;
    match cli.cmd {
        Some(Cmd::List { all, json: true }) => list_json(&project, all),
        Some(Cmd::List { all, json: false }) => list(&project, all),
        Some(Cmd::Show { id }) => {
            print!("{}", project.read(&project.resolve(id.as_deref())?.id)?);
            Ok(())
        }
        Some(Cmd::Rm { id, force }) => {
            let note = project.find(&id)?;
            if let (Some(pid), false) = (note.open_pid, force) {
                bail!(
                    "{} is open in another pane (pid {pid}); use --force",
                    note.id
                );
            }
            project.delete(&note.id)?;
            println!("deleted {}  {}", note.id, note.title);
            Ok(())
        }
        Some(Cmd::Path { dir: true, .. }) => {
            println!("{}", project.dir.display());
            Ok(())
        }
        Some(Cmd::Path { id, .. }) => {
            println!("{}", project.resolve(id.as_deref())?.path.display());
            Ok(())
        }
        None => {
            // `echo "..." | sidenote` captures stdin into a new note without opening the editor.
            if !cli.cont && cli.resume.is_none() && !std::io::stdin().is_terminal() {
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                if buf.trim().is_empty() {
                    bail!("nothing on stdin");
                }
                let id = project.new_id();
                project.write(&id, buf.trim_end())?;
                println!("{id}");
                return Ok(());
            }
            let start = match (cli.cont, cli.resume) {
                (true, _) => match project.list()?.first() {
                    Some(n) => tui::Start::Open(n.id.clone()),
                    None => tui::Start::New,
                },
                (_, Some(q)) if q.is_empty() => tui::Start::Pick,
                (_, Some(q)) => tui::Start::Open(project.find(&q)?.id),
                _ => tui::Start::New,
            };
            tui::run(project, start)
        }
    }
}

fn list(current: &Project, all: bool) -> Result<()> {
    let tty = std::io::stdout().is_terminal();
    let c = |code: &str, text: &str| {
        if tty {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    let projects = if all {
        Project::all()?
    } else {
        vec![Project::for_root(current.root.clone())]
    };
    let mut printed = false;
    for p in projects {
        let notes = p.list()?;
        if notes.is_empty() && all {
            continue;
        }
        if printed {
            println!();
        }
        printed = true;
        println!(
            "{}  {}",
            c("1", &p.name),
            c(
                "2",
                &format!("{} · {} notes", p.root.display(), notes.len())
            )
        );
        if notes.is_empty() {
            println!("  {}", c("2", "no notes yet — run `sidenote` to start one"));
        }
        for n in notes {
            let open = if n.open_pid.is_some() {
                c("33", "●")
            } else {
                " ".into()
            };
            println!(
                "{open} {}  {}  {}",
                c("36", &n.id),
                c("2", &format!("{:>9}", store::ago(n.modified))),
                n.title
            );
        }
    }
    if !printed {
        println!("no notes anywhere yet");
    }
    Ok(())
}

fn list_json(current: &Project, all: bool) -> Result<()> {
    let projects = if all {
        Project::all()?
    } else {
        vec![Project::for_root(current.root.clone())]
    };
    let mut out = Vec::new();
    for p in projects {
        for n in p.list()? {
            let modified = n
                .modified
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            out.push(serde_json::json!({
                "id": n.id,
                "title": n.title,
                "path": n.path,
                "modified": modified,
                "open": n.open_pid.is_some(),
                "repo": p.root,
            }));
        }
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
