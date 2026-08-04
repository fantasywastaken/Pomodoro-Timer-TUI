use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use notify_rust::Notification;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, Paragraph};
use ratatui::Frame;
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "pomodoro",
    version,
    about = "Terminal UI Pomodoro timer with statistics and desktop notifications"
)]
struct Cli {
    #[arg(long, default_value_t = 25, help = "Focus phase length in minutes")]
    focus: u64,
    #[arg(long = "short-break", default_value_t = 5, help = "Short break length in minutes")]
    short_break: u64,
    #[arg(long = "long-break", default_value_t = 15, help = "Long break length in minutes")]
    long_break: u64,
    #[arg(long = "long-break-every", default_value_t = 4, help = "Focus sessions before a long break")]
    long_break_every: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    fn label(&self) -> &'static str {
        match self {
            Phase::Focus => "FOCUS",
            Phase::ShortBreak => "SHORT BREAK",
            Phase::LongBreak => "LONG BREAK",
        }
    }
    fn color(&self) -> Color {
        match self {
            Phase::Focus => Color::Red,
            Phase::ShortBreak => Color::Green,
            Phase::LongBreak => Color::Cyan,
        }
    }
    fn key(&self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::ShortBreak => "ShortBreak",
            Phase::LongBreak => "LongBreak",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SessionRecord {
    date: DateTime<Local>,
    duration_secs: u64,
    phase: String,
}

struct App {
    cli: Cli,
    phase: Phase,
    remaining_secs: u64,
    total_secs: u64,
    paused: bool,
    completed_focus_in_cycle: usize,
    sessions: Vec<SessionRecord>,
    focus_seconds_today: u64,
    sessions_today: usize,
    streak: usize,
    quit: bool,
    log_path: PathBuf,
}

type Tui = ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut app = App::new(cli)?;
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run_app(terminal: &mut Tui, app: &mut App) -> Result<()> {
    let mut last_tick = Instant::now();
    while !app.quit {
        terminal.draw(|f| draw(f, app))?;
        let timeout = Duration::from_millis(250);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => app.quit = true,
                        KeyCode::Char(' ') => app.toggle_pause(),
                        KeyCode::Char('s') | KeyCode::Char('S') => app.skip()?,
                        _ => {}
                    }
                }
            }
        }
        let now = Instant::now();
        let elapsed = now.duration_since(last_tick);
        if elapsed >= Duration::from_secs(1) {
            let ticks = elapsed.as_secs();
            app.tick(ticks)?;
            last_tick = now;
        }
    }
    Ok(())
}

impl App {
    fn new(cli: Cli) -> Result<Self> {
        let log_path = session_log_path()?;
        let sessions = load_sessions(&log_path);
        let today = Local::now().date_naive();
        let focus_seconds_today: u64 = sessions
            .iter()
            .filter(|s| s.date.date_naive() == today && s.phase == "Focus")
            .map(|s| s.duration_secs)
            .sum();
        let sessions_today = sessions
            .iter()
            .filter(|s| s.date.date_naive() == today && s.phase == "Focus")
            .count();
        let streak = compute_streak(&sessions);
        let total_secs = cli.focus.saturating_mul(60);
        Ok(Self {
            phase: Phase::Focus,
            remaining_secs: total_secs,
            total_secs,
            paused: false,
            completed_focus_in_cycle: 0,
            sessions,
            focus_seconds_today,
            sessions_today,
            streak,
            quit: false,
            log_path,
            cli,
        })
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    fn tick(&mut self, seconds: u64) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        let step = seconds.min(self.remaining_secs);
        self.remaining_secs -= step;
        if matches!(self.phase, Phase::Focus) {
            self.focus_seconds_today += step;
        }
        if self.remaining_secs == 0 {
            self.complete_phase()?;
        }
        Ok(())
    }

    fn skip(&mut self) -> Result<()> {
        self.remaining_secs = 0;
        self.complete_phase()
    }

    fn complete_phase(&mut self) -> Result<()> {
        let record = SessionRecord {
            date: Local::now(),
            duration_secs: self.total_secs,
            phase: self.phase.key().to_string(),
        };
        self.sessions.push(record);
        if matches!(self.phase, Phase::Focus) {
            self.sessions_today += 1;
            self.completed_focus_in_cycle += 1;
        }
        let _ = save_sessions(&self.log_path, &self.sessions);
        let _ = self.notify_phase_end();
        self.transition();
        self.streak = compute_streak(&self.sessions);
        Ok(())
    }

    fn transition(&mut self) {
        match self.phase {
            Phase::Focus => {
                if self.completed_focus_in_cycle >= self.cli.long_break_every {
                    self.completed_focus_in_cycle = 0;
                    self.phase = Phase::LongBreak;
                    self.total_secs = self.cli.long_break.saturating_mul(60);
                } else {
                    self.phase = Phase::ShortBreak;
                    self.total_secs = self.cli.short_break.saturating_mul(60);
                }
            }
            Phase::ShortBreak | Phase::LongBreak => {
                self.phase = Phase::Focus;
                self.total_secs = self.cli.focus.saturating_mul(60);
            }
        }
        self.remaining_secs = self.total_secs;
    }

    fn notify_phase_end(&self) -> Result<()> {
        let (title, body) = match self.phase {
            Phase::Focus => ("Focus session complete", "Time to take a break."),
            Phase::ShortBreak => ("Short break over", "Ready for another focus session?"),
            Phase::LongBreak => ("Long break over", "Time to focus again."),
        };
        Notification::new()
            .summary(title)
            .body(body)
            .appname("Pomodoro Timer")
            .show()?;
        Ok(())
    }
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let outer = Block::default()
        .title(" Pomodoro Timer ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(app.phase.color()));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let phase_line = if app.paused {
        format!("Phase: {}   [PAUSED]", app.phase.label())
    } else {
        format!("Phase: {}", app.phase.label())
    };
    let phase_para = Paragraph::new(Line::from(vec![Span::styled(
        phase_line,
        Style::default()
            .fg(app.phase.color())
            .add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center);
    f.render_widget(phase_para, chunks[0]);

    let time_text = big_time(app.remaining_secs);
    let time_para = Paragraph::new(time_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(app.phase.color()));
    f.render_widget(time_para, chunks[1]);

    let ratio = if app.total_secs == 0 {
        0.0
    } else {
        let done = app.total_secs.saturating_sub(app.remaining_secs) as f64;
        (done / app.total_secs as f64).clamp(0.0, 1.0)
    };
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Progress "),
        )
        .gauge_style(Style::default().fg(app.phase.color()))
        .ratio(ratio);
    f.render_widget(gauge, chunks[2]);

    let stats = format!(
        "Today: {} sessions   |   Focus time: {}   |   Streak: {} day(s)",
        app.sessions_today,
        fmt_duration(app.focus_seconds_today),
        app.streak
    );
    let stats_para = Paragraph::new(stats).alignment(Alignment::Center);
    f.render_widget(stats_para, chunks[3]);

    let controls = Paragraph::new("[SPACE] Pause / Resume    [S] Skip    [Q] Quit")
        .alignment(Alignment::Center)
        .style(Style::default().add_modifier(Modifier::DIM));
    f.render_widget(controls, chunks[5]);
}

fn fmt_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

const DIGIT_ROWS: usize = 6;

fn digit_lines(c: char) -> [&'static str; DIGIT_ROWS] {
    match c {
        '0' => [
            " ████ ",
            "█    █",
            "█    █",
            "█    █",
            "█    █",
            " ████ ",
        ],
        '1' => [
            "  ██  ",
            " ███  ",
            "  ██  ",
            "  ██  ",
            "  ██  ",
            " ████ ",
        ],
        '2' => [
            " ████ ",
            "█    █",
            "    █ ",
            "   █  ",
            "  █   ",
            "██████",
        ],
        '3' => [
            " ████ ",
            "█    █",
            "   ██ ",
            "     █",
            "█    █",
            " ████ ",
        ],
        '4' => [
            "█    █",
            "█    █",
            "██████",
            "     █",
            "     █",
            "     █",
        ],
        '5' => [
            "██████",
            "█     ",
            "█████ ",
            "     █",
            "█    █",
            " ████ ",
        ],
        '6' => [
            " ████ ",
            "█     ",
            "█████ ",
            "█    █",
            "█    █",
            " ████ ",
        ],
        '7' => [
            "██████",
            "    █ ",
            "   █  ",
            "  █   ",
            " █    ",
            "█     ",
        ],
        '8' => [
            " ████ ",
            "█    █",
            " ████ ",
            "█    █",
            "█    █",
            " ████ ",
        ],
        '9' => [
            " ████ ",
            "█    █",
            "█    █",
            " █████",
            "     █",
            " ████ ",
        ],
        ':' => [
            "      ",
            "  ██  ",
            "      ",
            "      ",
            "  ██  ",
            "      ",
        ],
        _ => [
            "      ",
            "      ",
            "      ",
            "      ",
            "      ",
            "      ",
        ],
    }
}

fn big_time(secs: u64) -> Text<'static> {
    let mm = secs / 60;
    let ss = secs % 60;
    let s = format!("{:02}:{:02}", mm, ss);
    let chars: Vec<char> = s.chars().collect();
    let mut lines: Vec<Line> = Vec::with_capacity(DIGIT_ROWS);
    for row in 0..DIGIT_ROWS {
        let mut row_str = String::new();
        for (i, c) in chars.iter().enumerate() {
            if i > 0 {
                row_str.push(' ');
            }
            row_str.push_str(digit_lines(*c)[row]);
        }
        lines.push(Line::from(row_str));
    }
    Text::from(lines)
}

fn session_log_path() -> Result<PathBuf> {
    let home = home_dir().context("could not determine home directory")?;
    Ok(home.join(".pomodoro").join("sessions.json"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn load_sessions(path: &PathBuf) -> Vec<SessionRecord> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_sessions(path: &PathBuf, sessions: &[SessionRecord]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(sessions)?;
    fs::write(path, data)?;
    Ok(())
}

fn compute_streak(sessions: &[SessionRecord]) -> usize {
    let today = Local::now().date_naive();
    let days: std::collections::HashSet<NaiveDate> = sessions
        .iter()
        .filter(|s| s.phase == "Focus")
        .map(|s| s.date.date_naive())
        .collect();
    let mut streak = 0usize;
    let mut cur = today;
    while days.contains(&cur) {
        streak += 1;
        cur = match cur.pred_opt() {
            Some(p) => p,
            None => break,
        };
    }
    streak
}
