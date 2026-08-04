# Pomodoro Timer TUI

A fast, keyboard-driven Pomodoro timer that lives in your terminal. Built with `ratatui` and `crossterm`, it renders a countdown clock in large ASCII digits, tracks your daily focus statistics, and pings your desktop notification center whenever a phase ends.

## Features

- Configurable focus, short-break, and long-break durations (in minutes).
- Automatic cycle rotation: focus -> short break -> focus -> ... -> long break every N sessions.
- Native OS desktop notifications powered by `notify-rust`.
- Session history persisted to `~/.pomodoro/sessions.json`.
- Live statistics: today's total focus time, session count, and current daily streak.
- Big ASCII-digit countdown clock and colored progress bar.
- Non-blocking key handling: pause, skip, or quit at any moment.

## Installation

```bash
cd Pomodoro-Timer-TUI
cargo build --release
./target/release/pomodoro
```

## Usage

```bash
pomodoro                                      # default 25 / 5 / 15 minute cycles
pomodoro --focus 50 --short-break 10          # a 50/10 workflow
pomodoro --long-break 20 --long-break-every 3 # customise the long break rhythm
```

### CLI options

| Flag | Description | Default |
|------|-------------|---------|
| `--focus <MIN>` | Focus phase length in minutes | `25` |
| `--short-break <MIN>` | Short break length in minutes | `5` |
| `--long-break <MIN>` | Long break length in minutes | `15` |
| `--long-break-every <N>` | Focus sessions before a long break | `4` |

### Keyboard controls

| Key | Action |
|-----|--------|
| `Space` | Pause / resume the current phase |
| `S` | Skip the current phase and immediately move to the next one |
| `Q` | Quit the application |

## Data storage

Every completed phase is appended to `~/.pomodoro/sessions.json` on Unix or `%USERPROFILE%\.pomodoro\sessions.json` on Windows. The file is a simple JSON array of session records:

```json
[
  {
    "date": "2026-08-04T09:30:00+02:00",
    "duration_secs": 1500,
    "phase": "Focus"
  }
]
```

The current streak is computed as the number of consecutive days ending today that contain at least one completed focus session.

## Requirements

- Rust 1.74 or newer (edition 2021).
- A modern terminal with 256-color support and Unicode block characters.
- A working desktop notification service (D-Bus on Linux, Notification Center on macOS, Toast on Windows 10/11).

## License

Released under the MIT License.
