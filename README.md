# WayMeet

WayMeet is a Linux calendar tool with a CLI/daemon architecture. It reads
Google Calendar and CalDAV, shows upcoming meetings in the terminal or Waybar,
and supports quick meeting actions.

## Features

- Google Calendar and CalDAV provider support.
- Terminal, JSON, and Waybar outputs.
- Google event actions: edit in calendar, decline, and delete.
- Detects meeting links for Zoom, Meet, Teams, Webex, Jitsi, and more.
- Schedules desktop notifications with snooze support.
- Provides action commands for joining meetings, copying meeting details,
  refreshing providers, and creating meeting links.

## Installation

### From source

No releases and packages binaries are published yet. Install from source.

Install the CLI:

```sh
cargo install --path crates/waymeet-client
```

## Quick Start

### Google Calendar

Run the setup wizard:

```sh
waymeet auth google --guide
```

Manual setup:

1. Create a Google OAuth client of type `Desktop app` (a `Web
   application` client will fail with `redirect_uri_mismatch`).
2. Download the credentials JSON file.
3. Run:

```sh
waymeet auth google --account work \
  --credentials-file /path/to/client_secret_<id>.json
```

By default, waymeet requests `calendar.events` (read/write events) and
`calendar.readonly` (list calendars). This enables decline and delete actions.
Use `--read-only` (or `read_only = true` in account config) for read-only
access.

Authentication commands:

```sh
waymeet auth                          # starts Google guide when no providers are configured
waymeet auth status                   # inspect all accounts and tokens
waymeet auth logout --account work    # clear stored tokens
waymeet auth logout --revoke          # also revoke them with Google
waymeet auth google --no-browser      # headless/SSH flow, paste redirect URL in terminal
```

If you re-authenticate whilst the daemon is running, the CLI asks the daemon to
refresh with the new tokens.

WayMeet stores tokens in `~/.local/share/waymeet/` with `0600` permissions by
default. To use the desktop keyring instead (freedesktop Secret Service, via
`secret-tool` from `libsecret`), set
`token_storage = "keyring"` on the account.

### CalDAV

Add a CalDAV section to `~/.config/waymeet/config.toml`:

```toml
[caldav]
url = "https://caldav.example.com/calendars/chmouel/"
username = "env::WAYMEET_CALDAV_USER"
password = "env::WAYMEET_CALDAV_PASSWORD"
calendar_hint = "work"
```

### Run it

Show the next meeting:

```sh
waymeet
```

Use Waybar output:

```sh
waymeet --waybar
```

The CLI starts the daemon automatically when needed.

## Common Commands

- `waymeet --open-meet-url`
- `waymeet --copy-meeting-url`
- `waymeet --copy-meeting-id`
- `waymeet --copy-meeting-passcode`
- `waymeet --open-calendar-day`
- `waymeet --open-link-from-clipboard`
- `waymeet --create meet|zoom|teams|gcal`
- `waymeet --refresh`
- `waymeet --snooze N`

## Configuration

Default config path:

`~/.config/waymeet/config.toml`

Use `config.example.toml` as a template. Validate the resulting file with:

```sh
waymeet config validate
```

Supply Google credentials in either of these forms:

- `credentials_file = "/path/to/client_secret_<id>.json"` for the downloaded
  Google OAuth Desktop App JSON
- `client_id` and `client_secret` values, including `env::` and `pass::`
  references

CalDAV credentials also support `env::` and `pass::` references.

Tune the daemon cadence in the `[server]` section:

- `sync_interval_secs` — interval between calendar syncs (default 300, minimum 30)
- `refresh_cooldown_secs` — cooldown after a manual refresh (default 30)
- `notify_tick_secs` — interval between notification checks (default 30, minimum 5)

Notification checks run on their own ticker, independent of the sync interval,
so reminders can fire on time even when calendar syncs run less often.

### Notification timing

For Google Calendar events, WayMeet reads the event's own `reminders` setting
(popup reminders configured in Google Calendar, on by default) and uses those
minutes-before-start values before the global `notifications.minutes_before`
configuration. Explicit reminder overrides take precedence. If the event relies
on the calendar's default reminders, WayMeet resolves those defaults. WayMeet
falls back to `notifications.minutes_before` only when an event has disabled
reminders altogether, or when no provider reminder can be resolved (for example,
with non-Google providers).

## Environment Variables

- `WAYMEET_CONFIG`
- `WAYMEET_SOCKET`
- `GOOGLE_CLIENT_ID`
- `GOOGLE_CLIENT_SECRET`
- `GOOGLE_CREDENTIALS_FILE`
- `RUST_LOG`

## Architecture

The CLI communicates with a background daemon over a Unix socket. The daemon
handles provider polling, caching, and notifications.

For implementation details, see `DESIGN.md`.

## Licence

Apache-2.0
