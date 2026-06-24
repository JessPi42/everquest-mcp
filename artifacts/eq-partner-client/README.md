# EQ Partner — Desktop Client

A Windows command-line application that watches your EverQuest log file in real time and syncs parsed events to the EQ Compendium API.

## Features

- Monitors your EQ log file for new lines (seeks to EOF at startup, so only new events are captured)
- Parses: Loot, Kills, Quest completions, NPC dialogue, Spells, Skill-ups, Zone entries, /who
- Batches events (up to 200) and syncs to the API every 10 seconds
- Automatically retries once on sync failure
- Reads a simple JSON config file — no registry, no installer wizard
- Prints status lines to the console every 30 seconds

## Quick Start (Windows)

1. Download `eq-partner.exe` from the EQ Compendium Partner page (or build from source below).
2. Run: `eq-partner.exe --setup` and follow the prompts.
3. After setup, just run `eq-partner.exe` — keep the window open while playing EQ.

In-game, type `/log on` to enable logging. Your log file appears in `C:\EverQuest\Logs\`.

## Configuration

Settings are stored in `eq-partner-config.json` in the same directory as the exe.

```json
{
  "apiKey":      "eqp_...",
  "logFilePath": "C:\\EverQuest\\Logs\\eqlog_YourChar_Server.txt",
  "apiBaseUrl":  "https://your-app.replit.app/api"
}
```

| Field         | Description                                                              |
| ------------- | ------------------------------------------------------------------------ |
| `apiKey`      | Partner API Key from the Compendium admin panel (starts with `eqp_`)     |
| `logFilePath` | Full path to your EQ log file                                            |
| `apiBaseUrl`  | Your EQ Compendium deployment URL, e.g. `https://your-app.replit.app/api` |

**Get your API key:** Admin → System → Trusted Users → enable Partner toggle → Generate Key

## Command-Line Flags

```
eq-partner.exe                     # run with existing config
eq-partner.exe --setup             # interactive first-time setup
eq-partner.exe --config <path>     # use a custom config file path
```

## Build from Source

### Requirements

- Go 1.21+ (`go version`)

### Build for Windows (cross-compile from any OS)

```bash
cd artifacts/eq-partner-client/go-client
GOOS=windows GOARCH=amd64 go build -ldflags="-s -w" -o eq-partner.exe .
```

### Copy to the API server for distribution

```bash
# From the workspace root
pnpm build:partner-client
```

This cross-compiles `eq-partner.exe` and copies it to
`artifacts/api-server/public/partner/eq-partner.exe`, where it is served at
`/api/public/partner/eq-partner.exe`.

### Build for local testing (native Linux/macOS binary)

```bash
cd artifacts/eq-partner-client/go-client
go build -o eq-partner .
```

## Implementation Notes

- The Go client (`go-client/`) shares the same event parsing logic and API wire format as the Tauri prototype in `src-tauri/`, but is implemented in pure Go without any native GUI framework. This makes it straightforward to cross-compile for Windows from any host OS.
- The Tauri prototype in `src-tauri/` is retained as a reference for a future system-tray GUI version.
- Event parsing regex patterns mirror those in `src-tauri/src/log_parser.rs` exactly.
