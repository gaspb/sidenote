use crate::store::{self, Lock, Note, Project};
use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::SetTitle;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap};
use ratatui_textarea::{CursorMove, Input, Key, TextArea, WrapMode};
use std::io::Write;
use std::time::{Duration, Instant, SystemTime};

const AUTOSAVE_AFTER: Duration = Duration::from_millis(500);
const EXTERNAL_CHECK_EVERY: Duration = Duration::from_secs(1);
const TICK: Duration = Duration::from_millis(250);
/// Rows per entry in the picker list (title, meta, spacer).
const ITEM_HEIGHT: u16 = 3;

pub enum Start {
    New,
    Open(String),
    Pick,
}

pub fn run(project: Project, start: Start) -> Result<()> {
    let mut app = App::new(project);
    match start {
        Start::New => app.open_new()?,
        Start::Open(id) => app.open(&id)?,
        Start::Pick => app.show_picker()?,
    }
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    print!("\x1b[22;0t"); // push the terminal title so we can restore it
    let result = app.event_loop(&mut terminal);
    let closed = app.close_editor();
    print!("\x1b[23;0t"); // pop it
    if let Some(mut child) = herdr_title(None) {
        let _ = child.wait();
    }
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    result.and(closed)
}

struct Editor {
    id: String,
    textarea: TextArea<'static>,
    dirty: bool,
    last_edit: Instant,
    last_saved: Option<SystemTime>,
    disk_mtime: Option<SystemTime>,
    /// Text as last seen on disk: the common ancestor when merging outside edits.
    base: String,
    last_external_check: Instant,
    _lock: Lock,
}

impl Editor {
    fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Swap in new text, keeping the cursor on the same line if it still exists nearby.
    fn replace_text(&mut self, text: &str) {
        let (row, col) = (self.textarea.cursor().0, self.textarea.cursor().1);
        let current = self.textarea.lines().get(row).cloned();
        let mut textarea = new_textarea(text);
        let lines = textarea.lines();
        let target = current
            .filter(|l| !l.trim().is_empty())
            .and_then(|cur| {
                (0..lines.len())
                    .filter(|&i| lines[i] == cur)
                    .min_by_key(|&i| i.abs_diff(row))
            })
            .unwrap_or(row);
        textarea.move_cursor(CursorMove::Jump(target as u16, col as u16));
        self.textarea = textarea;
    }
}

fn normalize(s: &str) -> String {
    s.lines().collect::<Vec<_>>().join("\n")
}

#[derive(PartialEq)]
enum Mode {
    Edit,
    Pick,
}

struct Picker {
    notes: Vec<Note>,
    filter: String,
    state: ListState,
    confirm_delete: bool,
}

impl Picker {
    fn visible(&self) -> Vec<&Note> {
        let q = self.filter.to_lowercase();
        self.notes
            .iter()
            .filter(|n| {
                q.is_empty() || n.content.to_lowercase().contains(&q) || n.id.starts_with(&q)
            })
            .collect()
    }

    fn selected(&self) -> Option<&Note> {
        let i = self.state.selected()?;
        self.visible().get(i).copied()
    }

    fn clamp(&mut self) {
        let len = self.visible().len();
        let sel = match (len, self.state.selected()) {
            (0, _) => None,
            (_, None) => Some(0),
            (_, Some(i)) => Some(i.min(len - 1)),
        };
        self.state.select(sel);
    }
}

struct App {
    project: Project,
    mode: Mode,
    editor: Option<Editor>,
    picker: Picker,
    flash: Option<(String, Instant)>,
    quit: bool,
    /// Where the text and the picker list were last drawn, for mouse hit-testing.
    text_rect: Rect,
    list_rect: Rect,
    dragging: bool,
    /// Title last sent to the terminal / herdr.
    title: String,
}

impl App {
    fn new(project: Project) -> App {
        App {
            project,
            mode: Mode::Edit,
            editor: None,
            picker: Picker {
                notes: Vec::new(),
                filter: String::new(),
                state: ListState::default(),
                confirm_delete: false,
            },
            flash: None,
            quit: false,
            text_rect: Rect::default(),
            list_rect: Rect::default(),
            dragging: false,
            title: String::new(),
        }
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    // ── note lifecycle ────────────────────────────────────────────────

    fn open_new(&mut self) -> Result<()> {
        let id = self.project.new_id();
        self.load(id)
    }

    fn open(&mut self, id: &str) -> Result<()> {
        if self.editor.as_ref().is_some_and(|e| e.id == id) {
            self.mode = Mode::Edit;
            return Ok(());
        }
        if let Some(pid) = self.project.open_pid(id) {
            self.flash(format!(
                "open in another pane too (pid {pid}), edits are merged"
            ));
        }
        self.load(id.to_string())
    }

    fn load(&mut self, id: String) -> Result<()> {
        self.close_editor()?;
        let content = self.project.read(&id)?;
        let lock = self.project.lock(&id)?;
        let mut textarea = new_textarea(&content);
        textarea.move_cursor(CursorMove::Bottom);
        textarea.move_cursor(CursorMove::End);
        let disk_mtime = self.project.mtime(&id);
        self.editor = Some(Editor {
            id,
            textarea,
            dirty: false,
            last_edit: Instant::now(),
            last_saved: disk_mtime,
            disk_mtime,
            base: normalize(&content),
            last_external_check: Instant::now(),
            _lock: lock,
        });
        self.mode = Mode::Edit;
        Ok(())
    }

    fn save(&mut self) -> Result<()> {
        // Fold in anything written by someone else first, so we never clobber it.
        self.sync_external()?;
        let Some(ed) = self.editor.as_mut() else {
            return Ok(());
        };
        let text = ed.text();
        // Don't create files for notes that never had content.
        if text.trim().is_empty() && ed.disk_mtime.is_none() {
            ed.dirty = false;
            return Ok(());
        }
        let mtime = self.project.write(&ed.id, &text)?;
        ed.disk_mtime = Some(mtime);
        ed.last_saved = Some(mtime);
        ed.base = text;
        ed.dirty = false;
        Ok(())
    }

    /// Bring in changes another process (an agent, another pane) wrote to the file.
    /// Clean buffer: reload. Unsaved edits: 3-way merge against the last disk version.
    /// On conflict our text wins and theirs is kept as a separate note.
    fn sync_external(&mut self) -> Result<()> {
        let Some(ed) = self.editor.as_mut() else {
            return Ok(());
        };
        let now = self.project.mtime(&ed.id);
        if now.is_none() || now == ed.disk_mtime {
            return Ok(());
        }
        let theirs = normalize(&self.project.read(&ed.id)?);
        ed.disk_mtime = now;
        if theirs == ed.base {
            return Ok(());
        }
        let ours = ed.text();
        let msg = if !ed.dirty || ours == ed.base {
            ed.replace_text(&theirs);
            ed.dirty = false;
            ed.last_saved = now;
            "updated from another pane".to_string()
        } else {
            match diffy::merge(
                &format!("{}\n", ed.base),
                &format!("{ours}\n"),
                &format!("{theirs}\n"),
            ) {
                Ok(merged) => {
                    ed.replace_text(merged.strip_suffix('\n').unwrap_or(&merged));
                    "merged changes from another pane".to_string()
                }
                Err(_) => {
                    let id = self.project.new_id();
                    self.project.write(
                        &id,
                        &format!("# Conflicting copy of note {}\n\n{theirs}", ed.id),
                    )?;
                    format!("conflict: kept yours, theirs saved as note {id}")
                }
            }
        };
        ed.base = theirs;
        self.flash(msg);
        Ok(())
    }

    /// Save and release the current note; empty notes are removed.
    fn close_editor(&mut self) -> Result<()> {
        let Some(ed) = self.editor.as_ref() else {
            return Ok(());
        };
        if ed.text().trim().is_empty() {
            self.project.delete(&ed.id)?;
        } else if ed.dirty {
            self.save()?;
        }
        self.editor = None;
        Ok(())
    }

    fn show_picker(&mut self) -> Result<()> {
        self.save()?;
        self.picker.notes = self.project.list()?;
        self.picker.filter.clear();
        self.picker.confirm_delete = false;
        let current = self.editor.as_ref().map(|e| e.id.clone());
        let idx = current.and_then(|id| self.picker.notes.iter().position(|n| n.id == id));
        self.picker.state.select(idx.or(Some(0)));
        self.picker.clamp();
        self.mode = Mode::Pick;
        Ok(())
    }

    // ── loop ──────────────────────────────────────────────────────────

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(TICK)? {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k)?,
                    Event::Paste(s) => self.on_paste(&s),
                    Event::Mouse(m) => self.on_mouse(m)?,
                    _ => {}
                }
            }
            self.tick()?;
        }
        Ok(())
    }

    /// Terminal (and herdr pane) title: the note's saved title, or the repo in the picker.
    fn update_title(&mut self) {
        let title = match (&self.mode, &self.editor) {
            (Mode::Edit, Some(ed)) if !ed.base.trim().is_empty() => store::title_of(&ed.base),
            (Mode::Edit, _) => "new note".to_string(),
            (Mode::Pick, _) => format!("notes · {}", self.project.name),
        };
        if title == self.title {
            return;
        }
        let _ = execute!(std::io::stdout(), SetTitle(format!("✎ {title}")));
        if let Some(mut child) = herdr_title(Some(&title)) {
            std::thread::spawn(move || child.wait());
        }
        self.title = title;
    }

    fn tick(&mut self) -> Result<()> {
        self.update_title();
        if self
            .flash
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4))
        {
            self.flash = None;
        }
        let Some(ed) = self.editor.as_mut() else {
            return Ok(());
        };
        if ed.dirty && ed.last_edit.elapsed() >= AUTOSAVE_AFTER {
            if let Err(e) = self.save() {
                self.flash(format!("save failed: {e}"));
            }
            return Ok(());
        }
        if ed.last_external_check.elapsed() >= EXTERNAL_CHECK_EVERY {
            ed.last_external_check = Instant::now();
            self.sync_external()?;
        }
        Ok(())
    }

    fn on_paste(&mut self, s: &str) {
        match self.mode {
            Mode::Edit => {
                if let Some(ed) = self.editor.as_mut()
                    && ed
                        .textarea
                        .insert_str(s.replace("\r\n", "\n").replace('\r', "\n"))
                {
                    ed.dirty = true;
                    ed.last_edit = Instant::now();
                }
            }
            Mode::Pick => {
                self.picker.filter.push_str(s.lines().next().unwrap_or(""));
                self.picker.clamp();
            }
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) -> Result<()> {
        match self.mode {
            Mode::Edit => self.on_edit_mouse(m),
            Mode::Pick => self.on_pick_mouse(m)?,
        }
        Ok(())
    }

    /// Click to place the cursor, drag to select (copied to the clipboard on release), wheel to scroll.
    fn on_edit_mouse(&mut self, m: MouseEvent) {
        let area = self.text_rect;
        let Some(ed) = self.editor.as_mut() else {
            return;
        };
        let jump = |ed: &mut Editor| {
            let (top, left) = ed.textarea.scroll_offset();
            let row = m.row.saturating_sub(area.y) + top;
            let col = m.column.saturating_sub(area.x) + left;
            let p = ed.textarea.screen_to_data(row as usize, col as usize);
            ed.textarea
                .move_cursor(CursorMove::Jump(p.0 as u16, p.1 as u16));
        };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left)
                if area.contains(Position::new(m.column, m.row)) =>
            {
                ed.textarea.cancel_selection();
                jump(ed);
                self.dragging = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                if !ed.textarea.is_selecting() {
                    ed.textarea.start_selection();
                }
                jump(ed);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                if let Some(text) = selected_text(&ed.textarea).filter(|t| !t.is_empty()) {
                    copy_to_clipboard(&text);
                }
            }
            MouseEventKind::ScrollDown => ed.textarea.scroll((3, 0)),
            MouseEventKind::ScrollUp => ed.textarea.scroll((-3, 0)),
            _ => {}
        }
    }

    /// Click selects a note, clicking the selected one opens it; wheel moves the selection.
    fn on_pick_mouse(&mut self, m: MouseEvent) -> Result<()> {
        let p = &mut self.picker;
        match m.kind {
            MouseEventKind::Down(MouseButton::Left)
                if self.list_rect.contains(Position::new(m.column, m.row)) =>
            {
                let idx = p.state.offset() + ((m.row - self.list_rect.y) / ITEM_HEIGHT) as usize;
                if idx >= p.visible().len() {
                    return Ok(());
                }
                if p.state.selected() == Some(idx) {
                    let id = p.visible()[idx].id.clone();
                    self.open(&id)?;
                } else {
                    p.confirm_delete = false;
                    p.state.select(Some(idx));
                }
            }
            MouseEventKind::ScrollDown => {
                p.state
                    .select(Some(p.state.selected().map_or(0, |i| i + 1)));
                p.clamp();
            }
            MouseEventKind::ScrollUp => {
                p.state
                    .select(Some(p.state.selected().unwrap_or(0).saturating_sub(1)));
            }
            _ => {}
        }
        Ok(())
    }

    fn on_key(&mut self, k: KeyEvent) -> Result<()> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match self.mode {
            Mode::Edit => self.on_edit_key(k, ctrl),
            Mode::Pick => self.on_pick_key(k, ctrl),
        }
    }

    fn on_edit_key(&mut self, k: KeyEvent, ctrl: bool) -> Result<()> {
        match (k.code, ctrl) {
            (KeyCode::Esc, _)
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.textarea.is_selecting()) =>
            {
                if let Some(ed) = self.editor.as_mut() {
                    ed.textarea.cancel_selection();
                }
            }
            (KeyCode::Char('q'), true) | (KeyCode::Esc, _) => self.quit = true,
            (KeyCode::Char('s'), true) => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.dirty = true;
                }
                self.save()?;
                self.flash("saved");
            }
            (KeyCode::Char('o'), true) => self.show_picker()?,
            // Standard editor keys instead of the widget's emacs ones (^V = page down, ^A = line start).
            (KeyCode::Char('v'), true) => match read_clipboard() {
                Some(text) => self.on_paste(&text),
                None => self
                    .flash("clipboard is empty or unreadable (needs wl-paste, xclip or pbpaste)"),
            },
            (KeyCode::Char('a'), true) => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.textarea.select_all();
                }
            }
            (KeyCode::Char('t'), true) => {
                self.open_new()?;
                self.flash("new note");
            }
            _ => {
                let Some(ed) = self.editor.as_mut() else {
                    return Ok(());
                };
                let mut input = Input::from(k);
                // ^Z undo, ^Y / ^⇧Z redo (the widget's own keys are ^U / ^R).
                let remap = match input.key {
                    Key::Char('z') if input.ctrl && !input.shift => Some('u'),
                    Key::Char('Z' | 'z') if input.ctrl => Some('r'),
                    Key::Char('y') if input.ctrl => Some('r'),
                    _ => None,
                };
                if let Some(c) = remap {
                    input = Input {
                        key: Key::Char(c),
                        shift: false,
                        ..input
                    };
                }
                let copies = input.ctrl && matches!(input.key, Key::Char('c') | Key::Char('x'));
                if ed.textarea.input(input) {
                    ed.dirty = true;
                    ed.last_edit = Instant::now();
                }
                if copies && !ed.textarea.yank_text().is_empty() {
                    copy_to_clipboard(&ed.textarea.yank_text());
                }
            }
        }
        Ok(())
    }

    fn on_pick_key(&mut self, k: KeyEvent, ctrl: bool) -> Result<()> {
        let p = &mut self.picker;
        if p.confirm_delete {
            p.confirm_delete = false;
            if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'))
                && let Some(note) = p.selected().cloned()
            {
                if self.editor.as_ref().is_some_and(|e| e.id == note.id) {
                    self.editor = None;
                }
                self.project.delete(&note.id)?;
                self.picker.notes.retain(|n| n.id != note.id);
                self.picker.clamp();
                self.flash(format!("deleted “{}”", note.title));
            }
            return Ok(());
        }
        match (k.code, ctrl) {
            (KeyCode::Esc, _) | (KeyCode::Char('o'), true) => {
                if !p.filter.is_empty() {
                    p.filter.clear();
                    p.clamp();
                } else if self.editor.is_some() {
                    self.mode = Mode::Edit;
                } else {
                    self.quit = true;
                }
            }
            (KeyCode::Char('q') | KeyCode::Char('c'), true) => self.quit = true,
            (KeyCode::Char('t') | KeyCode::Char('n'), true) => {
                self.open_new()?;
            }
            (KeyCode::Char('d'), true) | (KeyCode::Delete, _) => {
                if p.selected().is_some() {
                    p.confirm_delete = true;
                }
            }
            (KeyCode::Enter, _) => {
                if let Some(id) = p.selected().map(|n| n.id.clone()) {
                    self.open(&id)?;
                } else if p.notes.is_empty() {
                    self.open_new()?;
                }
            }
            (KeyCode::Up, _) | (KeyCode::Char('p') | KeyCode::Char('k'), true) => {
                let i = p.state.selected().unwrap_or(0);
                p.state.select(Some(i.saturating_sub(1)));
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), true) => {
                let i = p.state.selected().map_or(0, |i| i + 1);
                p.state.select(Some(i));
                p.clamp();
            }
            (KeyCode::Backspace, _) => {
                p.filter.pop();
                p.clamp();
            }
            (KeyCode::Char(c), false) => {
                p.filter.push(c);
                p.state.select(Some(0));
                p.clamp();
            }
            _ => {}
        }
        Ok(())
    }

    // ── drawing ───────────────────────────────────────────────────────

    fn draw(&mut self, f: &mut Frame) {
        let [body, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        match self.mode {
            Mode::Edit => {
                if let Some(ed) = self.editor.as_ref() {
                    let inner = Block::new().padding(Padding::horizontal(1)).inner(body);
                    self.text_rect = inner;
                    f.render_widget(&ed.textarea, inner);
                }
            }
            Mode::Pick => self.draw_picker(f, body),
        }
        self.draw_status(f, status);
    }

    fn draw_picker(&mut self, f: &mut Frame, area: Rect) {
        let [head, list_area] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
        let visible: Vec<Note> = self.picker.visible().into_iter().cloned().collect();

        let count = if self.picker.filter.is_empty() {
            format!("{} notes", self.picker.notes.len())
        } else {
            format!("{}/{}", visible.len(), self.picker.notes.len())
        };
        let header = Line::from(vec![
            Span::raw(" "),
            Span::styled(&self.project.name, Style::new().bold()),
            Span::styled(format!("  {count}"), Style::new().dark_gray()),
        ]);
        let search = Line::from(vec![
            Span::styled(" › ", Style::new().fg(Color::Cyan)),
            Span::raw(&self.picker.filter),
            Span::styled("▏", Style::new().fg(Color::Cyan)),
            if self.picker.filter.is_empty() {
                Span::styled("type to filter", Style::new().dark_gray())
            } else {
                Span::raw("")
            },
        ]);
        f.render_widget(Paragraph::new(vec![header, search]), head);

        let wide = list_area.width >= 90;
        let [left, right] = if wide {
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(list_area)
        } else {
            [list_area, Rect::default()]
        };

        if visible.is_empty() {
            let msg = if self.picker.notes.is_empty() {
                "  No notes yet — press ↵ or ^T to start one"
            } else {
                "  No matches"
            };
            f.render_widget(Paragraph::new(msg).dark_gray(), left);
        } else {
            let current = self.editor.as_ref().map(|e| e.id.as_str());
            let items: Vec<ListItem> = visible
                .iter()
                .map(|n| {
                    let mut meta = vec![Span::styled(
                        format!("   {} · {}", n.id, store::ago(n.modified)),
                        Style::new().dark_gray(),
                    )];
                    if current == Some(n.id.as_str()) {
                        meta.push(Span::styled(" · editing", Style::new().fg(Color::Cyan)));
                    } else if n.open_pid.is_some() {
                        meta.push(Span::styled(
                            " · open in another pane",
                            Style::new().fg(Color::Yellow),
                        ));
                    }
                    ListItem::new(vec![
                        Line::from(format!(" {}", n.title)),
                        Line::from(meta),
                        Line::raw(""),
                    ])
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::new()
                        .bg(Color::Indexed(236))
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▌");
            self.list_rect = left;
            f.render_stateful_widget(list, left, &mut self.picker.state);
        }

        if wide && let Some(n) = self.picker.selected() {
            let preview = Paragraph::new(n.content.as_str())
                .wrap(Wrap { trim: false })
                .block(
                    Block::new()
                        .borders(Borders::LEFT)
                        .border_style(Style::new().dark_gray())
                        .padding(Padding::horizontal(2)),
                );
            f.render_widget(preview, right);
        }
    }

    fn draw_status(&self, f: &mut Frame, area: Rect) {
        let dim = Style::new().dark_gray();
        let mut left = vec![
            Span::styled(" sidenote ", Style::new().black().on_cyan().bold()),
            Span::raw(" "),
        ];
        left.push(Span::styled(self.project.name.clone(), Style::new().bold()));

        let (hints, state): (&str, Span) = match self.mode {
            Mode::Edit => {
                let ed = self.editor.as_ref();
                if let Some(ed) = ed {
                    left.push(Span::styled(format!("  {}", ed.id), dim));
                }
                let state = match ed {
                    Some(ed) if ed.dirty => {
                        Span::styled("● editing", Style::new().fg(Color::Yellow))
                    }
                    Some(Editor {
                        last_saved: Some(t),
                        ..
                    }) => Span::styled(format!("saved {}", store::ago(*t)), dim),
                    _ => Span::styled("new", dim),
                };
                ("^O notes  ^T new  ^Q quit", state)
            }
            Mode::Pick if self.picker.confirm_delete => (
                "",
                Span::styled("delete this note? y/n", Style::new().fg(Color::Red).bold()),
            ),
            Mode::Pick => ("↵ open  ^T new  ^D delete  esc back", Span::raw("")),
        };

        if let Some((msg, _)) = &self.flash {
            left.push(Span::styled(
                format!("  {msg}"),
                Style::new().fg(Color::Cyan),
            ));
        }
        let right = Line::from(vec![
            Span::raw("  "),
            state,
            Span::styled(format!("   {hints} "), dim),
        ])
        .right_aligned();
        let right_w = right.width() as u16;
        let [l, r] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
        f.render_widget(Line::from(left), l);
        f.render_widget(right, r);
    }
}

fn new_textarea(content: &str) -> TextArea<'static> {
    let lines: Vec<String> = content.lines().map(String::from).collect();
    let mut ta = TextArea::new(lines);
    ta.set_wrap_mode(WrapMode::Word);
    ta.set_cursor_line_style(Style::default());
    ta.set_placeholder_text("Start typing… the first line becomes the title. Saved automatically.");
    ta.set_placeholder_style(Style::new().dark_gray());
    ta
}

/// The selected text, without consuming the selection (unlike `TextArea::copy`).
fn selected_text(ta: &TextArea) -> Option<String> {
    let ((sr, sc), (er, ec)) = ta.selection_range()?;
    let lines = ta.lines();
    let chars = |row: usize, from: usize, to: usize| -> String {
        lines[row]
            .chars()
            .skip(from)
            .take(to.saturating_sub(from))
            .collect()
    };
    if sr == er {
        return Some(chars(sr, sc, ec));
    }
    let mut out = vec![chars(sr, sc, usize::MAX)];
    out.extend(lines[sr + 1..er].iter().cloned());
    out.push(chars(er, 0, ec));
    Some(out.join("\n"))
}

/// Put text on the system clipboard: via OSC 52, which most terminals (and tmux with
/// `set-clipboard on`) honour, even over SSH; and via the local clipboard tool if there is one.
fn copy_to_clipboard(text: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
    for (cmd, args) in clipboard_tools(true) {
        let child = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            std::thread::spawn(move || child.wait());
            return;
        }
    }
}

fn read_clipboard() -> Option<String> {
    clipboard_tools(false).into_iter().find_map(|(cmd, args)| {
        let out = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        (out.status.success() && !text.is_empty()).then_some(text)
    })
}

/// Clipboard commands to try, in order, for writing (`copy`) or reading.
fn clipboard_tools(copy: bool) -> Vec<(&'static str, &'static [&'static str])> {
    let mut tools: Vec<(&str, &[&str])> = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        tools.push(if copy {
            ("wl-copy", &[])
        } else {
            ("wl-paste", &["--no-newline"])
        });
    }
    if std::env::var_os("DISPLAY").is_some() {
        tools.push(if copy {
            ("xclip", &["-selection", "clipboard"])
        } else {
            ("xclip", &["-selection", "clipboard", "-o"])
        });
    }
    if cfg!(target_os = "macos") {
        tools.push(if copy {
            ("pbcopy", &[])
        } else {
            ("pbpaste", &[])
        });
    }
    tools
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Inside herdr, report the pane title (or clear it with `None`). Returns the spawned process.
fn herdr_title(title: Option<&str>) -> Option<std::process::Child> {
    if std::env::var("HERDR_ENV").ok()? != "1" {
        return None;
    }
    let pane = std::env::var("HERDR_PANE_ID").ok()?;
    let mut cmd = std::process::Command::new("herdr");
    cmd.args(["pane", "report-metadata", "--source", "sidenote"]);
    match title {
        Some(t) => cmd.args(["--title", t]),
        None => cmd.arg("--clear-title"),
    };
    cmd.arg(pane)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}
