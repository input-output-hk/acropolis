# Acropolis Monitor CLI

A Terminal User Interface (TUI) monitoring tool for tracking the health and performance of Acropolis processes.

## Features

- **Real-time Monitoring**: Automatically refreshes data at configurable intervals
- **Health Status Tracking**: Color-coded health indicators (Healthy/Warning/Critical)
- **Multiple Views**: Summary, Bottlenecks, and Data Flow visualization
- **Search & Filter**: Quickly find specific modules or topics
- **Sorting**: Customizable column sorting in all views
- **Historical Trends**: Sparkline charts showing read/write activity
- **Export**: Save current state to JSON for further analysis
- **Theme Support**: Auto-detects terminal light/dark mode

## Installation

Build from the workspace root:

```bash
cargo build -p acropolis_monitor_cli --release
```

The binary will be available at `target/release/monitor-cli`.

## Usage

```bash
monitor-cli [OPTIONS]
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `-f, --file <FILE>` | `monitor.json` | Path to the monitor.json data file |
| `-r, --refresh <SECS>` | `1` | Refresh interval in seconds |
| `--pending-warn <DURATION>` | `1s` | Pending duration warning threshold |
| `--pending-crit <DURATION>` | `10s` | Pending duration critical threshold |
| `--unread-warn <COUNT>` | `1000` | Unread message count warning threshold |
| `--unread-crit <COUNT>` | `5000` | Unread message count critical threshold |
| `-e, --export <FILE>` | - | Export state to JSON and exit (non-interactive) |

### Examples

```bash
# Monitor with default settings
monitor-cli -f /path/to/monitor.json

# Custom thresholds for sensitive monitoring
monitor-cli -f monitor.json --pending-warn 500ms --pending-crit 2s

# Export current state without TUI
monitor-cli -f monitor.json --export status.json

# Fast refresh rate for debugging
monitor-cli -f monitor.json -r 0.5
```

## Views

### Summary View (Tab 1)

Displays a table of all monitored modules with:
- **Module**: Name of the module
- **Reads**: Total messages read
- **Rate**: Read rate (messages/second)
- **Writes**: Total messages written
- **Pending**: Maximum pending duration across topics
- **Unread**: Total unread message count
- **Trend**: Sparkline showing recent read activity
- **Status**: Health indicator (Healthy/Warning/Critical)

### Bottlenecks View (Tab 2)

Lists all topics that are in Warning or Critical state:
- Sorted by severity (Critical first by default)
- Shows module name, topic, type (Read/Write), pending duration, and unread count
- Displays "All systems healthy!" when no issues exist

### Data Flow View (Tab 3)

Visualizes module-to-module communication:
- **Adjacency Matrix**: Shows relationships between all modules
  - `->` Module sends data to another
  - `<-` Module receives data from another
  - `<->` Bidirectional communication
- **Connection Detail**: Lists all connections for the selected module

## Keyboard Controls

### Navigation
| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch between views |
| `1`, `2`, `3` | Jump to specific view |
| `Up` / `k` | Move selection up |
| `Down` / `j` | Move selection down |
| `Page Up` | Move selection up by 10 |
| `Page Down` | Move selection down by 10 |
| `Home` | Jump to first item |
| `End` | Jump to last item |
| `Enter` | Open detail overlay |
| `Esc` / `Backspace` | Go back / close overlay |

### Search & Sort
| Key | Action |
|-----|--------|
| `/` | Start filter/search |
| `c` | Clear current filter |
| `s` | Cycle sort column |
| `S` | Toggle sort direction (asc/desc) |

### General
| Key | Action |
|-----|--------|
| `r` | Reload data from file |
| `e` | Export current state to JSON |
| `?` | Toggle help overlay |
| `q` | Quit |

### Mouse Controls
- **Scroll wheel**: Navigate up/down
- **Left click**: Select item or switch tabs
- **Right click**: Go back

## Health Status Thresholds

The tool evaluates health based on two metrics:

1. **Pending Duration**: How long messages have been waiting
   - Warning: >= `--pending-warn` (default: 1s)
   - Critical: >= `--pending-crit` (default: 10s)

2. **Unread Count**: Number of unread messages (reads only)
   - Warning: >= `--unread-warn` (default: 1000)
   - Critical: >= `--unread-crit` (default: 5000)

A module's overall health is the **worst** status across all its topics.

## Monitor Data Format

The tool expects a `monitor.json` file with the following structure:

```json
{
  "ModuleName": {
    "reads": {
      "TopicName": {
        "read": 12345,
        "pending_for": "1.234s",
        "unread": 100
      }
    },
    "writes": {
      "TopicName": {
        "written": 67890,
        "pending_for": "0.5s"
      }
    }
  }
}
```

### Field Descriptions

- **reads**: Map of topics this module reads from
  - `read`: Total messages read
  - `pending_for`: Duration since oldest pending message (optional)
  - `unread`: Number of unread messages (optional)
- **writes**: Map of topics this module writes to
  - `written`: Total messages written
  - `pending_for`: Duration since oldest pending write (optional)

Duration values support: `ns`, `us`/`µs`, `ms`, `s` (e.g., `"1.5s"`, `"500ms"`)

## Export Format

When using `-e/--export` or pressing `e` in the TUI, the tool outputs:

```json
{
  "summary": {
    "total_modules": 10,
    "healthy": 7,
    "warning": 2,
    "critical": 1,
    "total_reads": 1234567,
    "total_writes": 987654
  },
  "modules": [
    {
      "name": "ModuleName",
      "total_read": 12345,
      "total_written": 67890,
      "health": "Healthy",
      "reads": [...],
      "writes": [...]
    }
  ],
  "bottlenecks": [
    {
      "module": "ModuleName",
      "topic": "TopicName",
      "status": "Warning",
      "pending_for": "1.5s"
    }
  ]
}
```

## Tips

- Use **Bottlenecks view** for quick triage of issues
- Press `s` multiple times to cycle through sort columns
- The **sparkline** in Summary view shows read activity trends over the last 60 samples
- **Data Flow view** helps understand module dependencies and data pipelines
- Set tighter thresholds (`--pending-warn`, etc.) for early warning detection
