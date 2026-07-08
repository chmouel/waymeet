# waymeet Design

This document defines the architecture, runtime behaviour, and engineering
conventions for the waymeet repository.

## 1. High-Level Architecture

`waymeet` is a Rust workspace with a client/server design:

- `waymeet-client` (`waymeet` binary):
  - CLI entrypoint.
  - Connects to daemon over Unix socket.
  - Auto-spawns daemon when unavailable.
  - Renders TTY/Waybar/JSON output.
  - Executes user actions (open/copy URLs, refresh, snooze, create meeting links).
- `waymeet-server`:
  - Long-running daemon.
  - Holds in-memory meeting/provider state.
  - Schedules periodic provider sync with jitter + exponential backoff.
  - Serves IPC requests over Unix domain socket.
  - Sends desktop notifications.
- `waymeet-providers`:
  - Provider abstraction (`CalendarProvider` trait).
  - Google and CalDAV provider implementations.
  - Normalisation from provider-specific raw events to canonical events.
- `waymeet-core`:
  - Canonical event/time/link models.
  - Meeting link detection and normalisation across meeting providers.
  - Output formatting for terminal, Waybar, and JSON.
- `waymeet-protocol`:
  - IPC request/response schema.
  - Framing: 4-byte big-endian length + JSON envelope.
  - Protocol constants (`PROTOCOL_VERSION = "1"`, max message size 1MB).

## 2. Runtime Data Flow

1. User runs `waymeet`.
2. CLI loads config (`~/.config/waymeet/config.toml` by default).
3. CLI builds a `Request` and calls daemon over Unix socket.
4. If connection fails, CLI auto-spawns `waymeet server`, waits for readiness, retries request.
5. Server request handler returns meetings/status/acknowledgements.
6. CLI either:
   - prints formatted output (TTY/Waybar/JSON), or
   - performs action (open URL, copy URL/ID/passcode, open calendar day, refresh, snooze, create).

Server background loop:

1. Build configured providers.
2. Scheduler triggers sync.
3. Each provider fetches `RawEvent`.
4. `normalize_events` converts to `NormalizedEvent`.
5. `MeetingView::from_event` produces display-ready meetings.
6. Server state is updated per provider (last-known-good data survives provider errors and not-modified responses) and merged sorted.
7. A dedicated notification ticker (default 30 s, woken immediately after each sync) checks thresholds and emits deduplicated notifications, independently of the sync cadence.

## 3. Crate Responsibilities

### 3.1 `waymeet-client`

Key modules:

- `src/main.rs`: CLI execution, auto-spawn, filter building, rendering, status output.
- `src/cli.rs`: Clap schema (flags and subcommands).
- `src/actions.rs`: local user actions and request-based actions (`refresh`, `snooze`). Also holds `edit_calendar_event_url`, currently unused by any CLI path (see §9).
- `src/socket.rs`: Unix socket request/response exchange with request correlation.
- `src/commands/mod.rs`: command module registration.
- `src/commands/auth.rs`: Google auth command and credential persistence.
- `src/commands/config.rs`: `config dump`/`config validate` implementations.
- `src/commands/server.rs`: daemon bootstrap orchestration — builds providers, wires the scheduler, notification ticker, and socket server, and constructs the `EventMutator` closure that dispatches `MutateEvent` requests to `CalendarProvider::mutate_event`.
- `src/config.rs`: config schema + XDG paths + secret reference handling.
- `src/secret.rs`: credential reference resolution (`pass::`, `env::`, plain text).

### 3.2 `waymeet-server`

Key modules:

- `scheduler.rs`: interval sync, jitter, cooldown, jittered capped exponential backoff (never gives up permanently: past the failure ceiling it keeps probing at the capped cadence, and forced refreshes always run), commands (`Refresh`, `SyncNow`, `Pause`, `Resume`, `Stop`).
- `handler.rs`: protocol request dispatch (`Ping`, `Status`, `GetMeetings`, `Refresh`, `Snooze`, `MutateEvent`, `Shutdown`), filter application, and per-provider meeting state (last-known-good retention).
- `socket.rs`: Unix listener, framed protocol I/O, stale socket cleanup, connection concurrency limit.
- `notify.rs`: desktop notifications, dedup hash (`SHA-256`) with time-based eviction (24 h retention, size-capped oldest-first), optional morning agenda.
- `signals.rs`: `SIGTERM`/`SIGINT` shutdown and `SIGHUP` reload signalling.
- `pidfile.rs`: duplicate-instance guard.
- `cache.rs`: TTL cache utilities (currently generic infra; server state currently uses in-memory meetings directly).

### 3.3 `waymeet-providers`

- `provider.rs`: `CalendarProvider` trait + `FetchOptions` + `FetchResult` + `mutate_event` (decline/delete).
- `normalize.rs`: raw-to-normalized conversion pipeline, link extraction precedence.
- `google/`: OAuth2 PKCE loopback auth (`oauth.rs`), token refresh/storage (`tokens.rs`), Calendar API fetch (`client.rs`, `provider.rs`), account config (`config.rs`).
- `caldav/`: calendar discovery via PROPFIND and event fetch via REPORT (`client.rs`, `provider.rs`), Basic/Digest auth (`auth.rs`), ICS parsing (`ics.rs`), XML handling (`xml.rs`), account config (`config.rs`).

### 3.4 `waymeet-core`

- `event.rs`: canonical event types (`NormalizedEvent`, `MeetingView`, link and attendee metadata).
- `links.rs`: URL extraction, SafeLinks unwrapping, service classification across meeting providers (Zoom, Meet, Teams, Webex, Jitsi, and others).
- `format/mod.rs`: formatter for TTY, Waybar JSON, structured JSON output; `format/golden_tests.rs` holds the snapshot test suite.
- `time.rs`: time-window abstractions.

### 3.5 `waymeet-protocol`

- `types.rs`: `Envelope`, `Request`/`Response` (including `MutateEvent` with `EventMutationAction::{Decline, Delete}`), `MeetingsFilter`, provider status/state payloads.
- `framing.rs`: serialization with length-prefix framing.
- `error.rs`: protocol error model.

## 4. Command Surface (Current CLI)

Binary:

- `waymeet [OPTIONS] [COMMAND]`

Top-level options:

- `--config, -c <PATH>`
- `--debug, -v`
- `--waybar`
- `--json`
- `--privacy`
- `--snooze <MINUTES>`
- `--open-meet-url`
- `--copy-meeting-url`
- `--copy-meeting-id`
- `--copy-meeting-passcode`
- `--open-calendar-day`
- `--open-link-from-clipboard`
- `--create <SERVICE>`
- `--create-url <URL>`
- `--refresh`
- `--socket-path <PATH>`

Subcommands:

- `waymeet auth google [...]`
- `waymeet auth status`
- `waymeet auth logout [--account <NAME>] [--revoke]`
- `waymeet config dump`
- `waymeet config validate`
- `waymeet config path`
- `waymeet status`
- `waymeet server`

Environment variables:

- `WAYMEET_CONFIG`
- `WAYMEET_SOCKET`
- `WAYMEET_DEBUG_LEVEL`
- `GOOGLE_CLIENT_ID`
- `GOOGLE_CLIENT_SECRET`
- `GOOGLE_CREDENTIALS_FILE`
- `RUST_LOG`

## 5. Configuration Model

Default config path:

- `~/.config/waymeet/config.toml`

Main sections:

- `[google]` / `[[google.accounts]]` (when Google feature enabled)
- `[caldav]` (when CalDAV feature enabled)
- `[display]`
- `[filters]`
- `[notifications]`
- `[server]`

Credential resolution supports:

- `pass::path/to/secret` (via `pass show`, first line)
- `env::VAR_NAME`
- plain text values

## 6. Implemented Features

### Core UX

- Client/daemon split with automatic daemon spawning.
- Unix socket IPC with request-response correlation IDs.
- Output modes: terminal, Waybar JSON, machine JSON.
- Status command with provider health.

### Providers

- Google Calendar provider:
  - OAuth 2.0 PKCE (loopback callback on an ephemeral port).
  - Desktop-app client detection: `web`-type credentials produce a
    warning, as Google rejects loopback redirects for them
    (`redirect_uri_mismatch`).
  - consent screen only forced when needed (`--force`, scope changes,
    or a missing refresh token, with one automatic consent retry).
  - default scopes `calendar.events` + `calendar.readonly`; per-account
    `read_only` and explicit `scopes` overrides.
  - headless flow (`--no-browser`): the pasted redirect URL is accepted
    in place of the loopback callback.
  - token refresh support with `invalid_grant` detection and explicit
    re-authentication guidance.
  - token storage backends: `file` (0600 JSON) or `keyring`
    (freedesktop Secret Service via `secret-tool`, with file fallback).
  - `auth status` / `auth logout [--revoke]` subcommands; a running
    daemon is nudged to reload tokens after re-authentication.
  - multi-account configuration.
  - per-event popup reminder minutes are read from the event's `reminders`
    field (explicit `overrides`, or the calendar's `defaultReminders` when
    `useDefault` is set); resolved calendar defaults are cached per calendar
    ID with a 1-hour TTL (`GoogleProvider::reminder_defaults_cache`) to
    avoid an extra `calendarList.list` call on every sync.
- CalDAV provider:
  - calendar discovery via PROPFIND.
  - Basic and Digest authentication.
  - ICS parsing pipeline.

### Filtering and Display

- Filters include:
  - today-only, limit, skip all-day
  - include/exclude title patterns
  - include/exclude calendar patterns
  - within-minutes
  - only-with-link
  - work-hours window
  - privacy mode/title masking
  - response-state filtering (declined/tentative/pending)
  - skip solo events
- Formatter capabilities:
  - truncation
  - relative vs absolute time handling
  - custom format templates
  - 12h/24h options
  - Waybar class + optional colour markup

### Meeting Links and Actions

- Extensive meeting URL detection and normalisation.
- Meeting action commands:
  - open next meeting URL
  - copy meeting URL
  - copy meeting ID
  - copy meeting passcode
  - open calendar day
  - open link from clipboard
  - create meeting URLs for meet/zoom/teams/gcal

### Notifications

- Configurable pre-meeting reminders (`minutes_before` list) act as the
  fallback when a per-event reminder can't be resolved.
- For Google Calendar events, `NormalizedEvent`/`MeetingView` carry a
  resolved `reminder_minutes: Option<Vec<u32>>` (popup-only minutes, from
  the event's own overrides or the calendar's default reminders); when
  present and non-empty, `NotifyEngine::due_start_soon_thresholds` uses
  these instead of the global `notify_minutes` config.
- Dedicated notification ticker decoupled from calendar sync (`server.notify_tick_secs`, default 30 s), woken immediately after each sync.
- Start-soon thresholds (e.g. `[15, 5, 1]`, or an event's own reminder minutes) have nesting windows; if a check is delayed and finds several thresholds due at once, only one notification is sent per meeting, labelled with the actual remaining time rather than the stale configured number.
- Deduplication based on notification hash, with time-based eviction so in-window alerts are never re-sent.
- Snooze via command/protocol; expired snoozes are pruned automatically.
- Optional morning agenda notification time.

## 7. Test Strategy (Current)

- Rust unit tests cover protocol, server, providers, and client command/logic paths.
- Formatter behaviour is validated with golden snapshot tests in `waymeet-core`.

## 8. Notable Implementation Characteristics

- Scheduler defaults: 5-minute sync interval, 10% jitter, cooldown + jittered capped exponential backoff; tunable via `[server]` (`sync_interval_secs`, `refresh_cooldown_secs`, `notify_tick_secs`).
- Protocol messages are versioned via envelope and have strict size caps.
- Socket startup handles stale socket cleanup and concurrent connection limits.
- A PID file prevents duplicate server instances.

## 9. Current Boundaries and Gaps

- The `waymeet-server` cache module exists and is tested, but active meeting serving currently relies on in-memory `ServerState` rather than direct `EventCache` integration.
- `SIGHUP` reload signal plumbing exists; full dynamic provider/config rebuild flow is not yet surfaced as a complete runtime reload path in server orchestration.
- Event mutation (decline/delete) is implemented end-to-end at the provider,
  protocol, and server level — `CalendarProvider::mutate_event`,
  `Request::MutateEvent`/`EventMutationAction`, and the `EventMutator` wired
  in `commands/server.rs` — but no CLI flag or subcommand invokes it yet.
  Likewise, `actions::edit_calendar_event_url` builds a valid
  "open event in Google Calendar editor" URL but isn't called from any
  command path. Both are backend-ready, CLI-unwired.

## 10. Repository Conventions

### 10.1 Documentation Style

- Use consistent British spelling.
- Keep a professional butler tone: clear, helpful, dignified, and not pompous.
- Avoid overly casual Americanisms.
- Maintain technical precision whilst preserving readability.

### 10.2 Quality Gates Before Finishing

- Run `cargo clippy --all --all-features --fix`.
- Add tests for new or changed functionality.
- Keep coverage expectations high.
