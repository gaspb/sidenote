mod store;
mod tui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Read};
use store::Project;

/// Scratch notes tied to the current repo, stored outside it.
///
/// Run `notes` to start a new note, `notes -c` to continue the latest one,
/// or `notes -r` to pick from this repo's notes. Notes autosave as you type.
#[derive(Parser)]
#[command(name = "notes", version)]
struct Cli {
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
    },
    /// Print a note to stdout
    #[command(visible_alias = "cat")]
    Show { id: String },
    /// Delete a note
    Rm {
        id: String,
        /// Delete even if it is open in another pane
        #[arg(short, long)]
        force: bool,
    },
    /// Print the storage directory for this repo (or a note's file path)
    Path { id: Option<String> },
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("notes: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let project = Project::current()?;
    match cli.cmd {
        Some(Cmd::List { all }) => list(&project, all),
        Some(Cmd::Show { id }) => {
            print!("{}", project.read(&project.find(&id)?.id)?);
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
        Some(Cmd::Path { id }) => {
            match id {
                Some(id) => println!("{}", project.find(&id)?.path.display()),
                None => println!("{}", project.dir.display()),
            }
            Ok(())
        }
        None => {
            // `echo "..." | notes` captures stdin into a new note without opening the editor.
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
            println!("  {}", c("2", "no notes yet — run `notes` to start one"));
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
