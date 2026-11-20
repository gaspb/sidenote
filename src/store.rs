use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT_FILE: &str = ".root";
const ID_ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Base directory for all notes: $SIDENOTE_HOME, else $XDG_DATA_HOME/sidenote, else ~/.local/share/sidenote.
pub fn data_home() -> PathBuf {
    if let Some(p) = std::env::var_os("SIDENOTE_HOME") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".local/share")
        });
    base.join("sidenote")
}

fn git(cwd: &Path, args: &[&str]) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// The project a directory belongs to: the main checkout of its git repo (so linked
/// worktrees share notes with it), else the directory itself.
pub fn resolve_root(cwd: &Path) -> PathBuf {
    if let Some(common) = git(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) && common.file_name().is_some_and(|n| n == ".git")
        && let Some(parent) = common.parent()
    {
        return parent.to_path_buf();
    }
    if let Some(top) = git(cwd, &["rev-parse", "--show-toplevel"]) {
        return top;
    }
    cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
}

fn encode(root: &Path) -> String {
    root.to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub struct Project {
    pub root: PathBuf,
    pub dir: PathBuf,
    pub name: String,
}

#[derive(Clone)]
pub struct Note {
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub content: String,
    pub modified: SystemTime,
    pub open_pid: Option<u32>,
}

impl Project {
    pub fn for_root(root: PathBuf) -> Project {
        let dir = data_home().join("projects").join(encode(&root));
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".into());
        Project { root, dir, name }
    }

    pub fn current() -> Result<Project> {
        let cwd = std::env::current_dir().context("cannot read current directory")?;
        Ok(Project::for_root(resolve_root(&cwd)))
    }

    pub fn all() -> Result<Vec<Project>> {
        let base = data_home().join("projects");
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&base) else {
            return Ok(out);
        };
        for e in entries.flatten() {
            if let Ok(root) = fs::read_to_string(e.path().join(ROOT_FILE)) {
                out.push(Project::for_root(PathBuf::from(root.trim())));
            }
        }
        out.sort_by(|a, b| a.root.cmp(&b.root));
        Ok(out)
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("cannot create {}", self.dir.display()))?;
        let marker = self.dir.join(ROOT_FILE);
        if !marker.exists() {
            fs::write(&marker, self.root.to_string_lossy().as_bytes())?;
        }
        Ok(())
    }

    pub fn note_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.md"))
    }

    /// Notes currently open in other live processes: id -> pid. Stale lock files are removed.
    fn open_elsewhere(&self) -> HashMap<String, u32> {
        let mut open = HashMap::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return open;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((id, pid)) = name.strip_suffix(".lock").and_then(|s| s.rsplit_once('.'))
            else {
                continue;
            };
            let Ok(pid) = pid.parse::<u32>() else {
                continue;
            };
            if !pid_alive(pid) {
                let _ = fs::remove_file(e.path());
            } else if pid != std::process::id() {
                open.insert(id.to_string(), pid);
            }
        }
        open
    }

    /// All notes, most recently modified first.
    pub fn list(&self) -> Result<Vec<Note>> {
        let mut notes = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Ok(notes);
        };
        let open = self.open_elsewhere();
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "md") {
                continue;
            }
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let content = fs::read_to_string(&path).unwrap_or_default();
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            notes.push(Note {
                title: title_of(&content),
                open_pid: open.get(&id).copied(),
                id,
                path,
                content,
                modified,
            });
        }
        notes.sort_by_key(|n| std::cmp::Reverse(n.modified));
        Ok(notes)
    }

    /// Find a note by exact id or unique id prefix.
    pub fn find(&self, query: &str) -> Result<Note> {
        let notes = self.list()?;
        if let Some(n) = notes.iter().find(|n| n.id == query) {
            return Ok(n.clone());
        }
        let matches: Vec<_> = notes.iter().filter(|n| n.id.starts_with(query)).collect();
        match matches.as_slice() {
            [n] => Ok((*n).clone()),
            [] => bail!("no note matching '{query}' in {}", self.name),
            _ => bail!(
                "'{query}' is ambiguous: {}",
                matches
                    .iter()
                    .map(|n| n.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// The note a bare `show`/`path` means: an explicit id, else the most recent note
    /// open in an editor, else the most recent note.
    pub fn resolve(&self, id: Option<&str>) -> Result<Note> {
        if let Some(id) = id {
            return self.find(id);
        }
        let notes = self.list()?;
        notes
            .iter()
            .find(|n| n.open_pid.is_some())
            .or(notes.first())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no notes yet in {}", self.name))
    }

    pub fn new_id(&self) -> String {
        loop {
            let id = random_id();
            if !self.note_path(&id).exists() {
                return id;
            }
        }
    }

    pub fn read(&self, id: &str) -> Result<String> {
        let p = self.note_path(id);
        match fs::read_to_string(&p) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", p.display())),
        }
    }

    /// Atomic write (temp file + rename). Returns the new mtime.
    pub fn write(&self, id: &str, content: &str) -> Result<SystemTime> {
        self.ensure_dir()?;
        let path = self.note_path(id);
        let tmp = self.dir.join(format!(".{id}.md.tmp"));
        let mut body = content.to_string();
        if !body.ends_with('\n') {
            body.push('\n');
        }
        fs::write(&tmp, body).with_context(|| format!("cannot write {}", tmp.display()))?;
        fs::rename(&tmp, &path)?;
        Ok(fs::metadata(&path)?.modified()?)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        match fs::remove_file(self.note_path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub fn mtime(&self, id: &str) -> Option<SystemTime> {
        fs::metadata(self.note_path(id))
            .and_then(|m| m.modified())
            .ok()
    }

    /// PID of a live process (other than us) that has this note open.
    pub fn open_pid(&self, id: &str) -> Option<u32> {
        self.open_elsewhere().get(id).copied()
    }

    pub fn lock(&self, id: &str) -> Result<Lock> {
        self.ensure_dir()?;
        let path = self.dir.join(format!("{id}.{}.lock", std::process::id()));
        fs::write(&path, "")?;
        Ok(Lock { path })
    }
}

/// Marks a note as open in this process; removed on drop.
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn pid_alive(pid: u32) -> bool {
    let proc = Path::new("/proc");
    if proc.exists() {
        return proc.join(pid.to_string()).exists();
    }
    // No procfs (macOS, BSD): `kill -0` probes the process without signalling it.
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn random_id() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    h.write_u32(std::process::id());
    let mut n = h.finish();
    (0..6)
        .map(|_| {
            let c = ID_ALPHABET[(n % 32) as usize] as char;
            n /= 32;
            c
        })
        .collect()
}

/// First non-empty line, minus markdown heading markers.
pub fn title_of(content: &str) -> String {
    content
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(80).collect())
        .unwrap_or_else(|| "(empty)".into())
}

pub fn ago(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .unwrap_or_default()
        .as_secs();
    match s {
        0..10 => "just now".into(),
        10..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        86400..2_592_000 => format!("{}d ago", s / 86400),
        2_592_000..31_536_000 => format!("{}mo ago", s / 2_592_000),
        _ => format!("{}y ago", s / 31_536_000),
    }
}
