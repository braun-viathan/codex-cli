# codex-app-server-daemon

> `codex-app-server-daemon` is experimental and its lifecycle contract may
> change while the remote-management flow is still being developed.

`codex-app-server-daemon` backs the machine-readable `codex app-server`
lifecycle commands used by remote clients such as the desktop and mobile apps.
It is intended for Codex instances launched over SSH, including fresh developer
machines that should expose app-server with `remote_control` enabled.

## Platform support

The current daemon implementation is Unix-only. It uses pidfile-backed
daemonization plus Unix process and file-locking primitives, and does not yet
support Windows lifecycle management.

## Commands

```sh
codex app-server daemon start
codex app-server daemon restart
codex app-server daemon enable-remote-control
codex app-server daemon disable-remote-control
codex app-server daemon stop
codex app-server daemon version
codex app-server daemon bootstrap --remote-control
codex app-server daemon bootstrap --remote-control --codex-bin /absolute/path/to/codex
codex app-server daemon bootstrap --managed
```

On success, every command writes exactly one JSON object to stdout. Consumers
should parse that JSON rather than relying on human-readable text. Lifecycle
responses report the resolved backend, socket path, local CLI version, and
running app-server version when applicable.

## Bootstrap flow

For a new remote machine:

```sh
curl -fsSL https://chatgpt.com/codex/install.sh | sh
$HOME/.codex/packages/standalone/current/codex app-server daemon bootstrap --remote-control
```

By default, `bootstrap` requires the standalone managed install. It records the
daemon settings under `CODEX_HOME/app-server-daemon/`, starts app-server as a
pidfile-backed detached process, and launches a detached updater loop.

Advanced local builds can opt out of the managed standalone binary explicitly:

```sh
codex app-server daemon bootstrap --remote-control --codex-bin /absolute/path/to/codex
```

That persists the custom binary path in
`CODEX_HOME/app-server-daemon/settings.json` and disables the detached
standalone updater for this daemon. Use `bootstrap --managed` to clear the
custom path and return to the managed standalone install.

## Installation and update cases

The daemon defaults to the standalone managed binary installed through
`install.sh`, but can be pinned to an explicit custom binary. Desktop app
updates are a separate app-bundle update path: on macOS, the Codex app can update
`Codex.app` and its bundled `Contents/Resources/codex`, but that does not rewrite
`CODEX_HOME/app-server-daemon/settings.json` or a persisted custom binary path.

| Situation | What starts | Does this daemon fetch new binaries? | Does a running app-server eventually move to a newer binary on its own? |
| --- | --- | --- | --- |
| `install.sh` has run, but only `start` is used | `start` uses `CODEX_HOME/packages/standalone/current/codex` | No | No. The managed path is used when starting or restarting, but no updater is installed. |
| `install.sh` has run, then `bootstrap` is used | The pidfile backend uses `CODEX_HOME/packages/standalone/current/codex` | Yes. Bootstrap launches a detached updater loop that runs `install.sh` hourly. | Yes, while that updater process is alive and app-server is already running. After a successful fetch, the updater restarts app-server with the refreshed binary and only then replaces its own process image. |
| `bootstrap --codex-bin /absolute/path/to/codex` is used | The pidfile backend uses the explicit custom binary path persisted in `settings.json` | No | No. Custom binary mode is user-managed; the daemon does not run the standalone updater and does not watch arbitrary executable files for replacement. |
| Some other tool updates the managed binary path | The next fresh start or restart uses the updated file at that path | Only if `bootstrap` is active, because the updater still runs `install.sh` on its normal cadence. | Without `bootstrap`, no. With `bootstrap`, the next successful updater pass compares the managed binary contents after `install.sh` runs; if app-server is running and they differ from the updater's current image, it refreshes app-server first and then itself. |

### Standalone installs

For installs created by `install.sh`:

- lifecycle commands always use the standalone managed binary path
- `bootstrap` is supported
- `bootstrap` starts a detached pid-backed updater loop that fetches via
  `install.sh`
- after a successful refresh, if app-server is running and the managed binary
  contents changed, the updater restarts app-server with that binary first and
  only then replaces its own process image
- the updater loop is not reboot-persistent; it must be started again by
  rerunning `bootstrap` after a reboot

### Custom binary installs

For explicit custom binaries selected with `bootstrap --codex-bin`:

- lifecycle commands use the persisted custom binary path
- `bootstrap --managed` clears the custom path and returns to the standalone
  managed install
- the standalone updater is disabled, and stale updater loops stop before
  fetching `install.sh`
- Desktop app bundle updates do not update or replace the custom daemon binary
- after a Desktop app update or host reboot, re-run `bootstrap --codex-bin` with
  the custom CLI before relying on Remote Control, because the updated app bundle
  may include a bundled CLI that does not know about local custom-binary support
- replacing the custom binary file is the caller's responsibility; restart the
  daemon to move a running app-server process to the new executable image

### Out-of-band updates

This daemon does not watch arbitrary executable files for replacement. If some
other tool updates the managed binary path:

- without `bootstrap`, a currently running app-server remains on the old
  executable image until an explicit `restart`
- with `bootstrap`, the detached updater loop notices the changed managed
  binary on its next successful scheduled pass after running `install.sh`; if
  app-server is running, it refreshes app-server first and then refreshes itself
  once that replacement starts successfully

## Lifecycle semantics

`start` is idempotent and returns after app-server is ready to answer the normal
JSON-RPC initialize handshake on the Unix control socket.

`restart` stops any managed daemon and starts it again.

`enable-remote-control` and `disable-remote-control` persist the launch setting
for future starts. If a managed app-server is already running, they restart it
so the new setting takes effect immediately.

Top-level `codex remote-control` bootstraps with `--remote-control` when the
updater loop is not running. Otherwise it enables remote control and starts the
daemon normally.

`stop` sends a graceful termination request first, then sends a second
termination signal after the grace window if the process is still alive.

All mutating lifecycle commands are serialized per `CODEX_HOME`, so a concurrent
`start`, `restart`, `enable-remote-control`, `disable-remote-control`, `stop`,
or `bootstrap` does not race another in-flight lifecycle operation.

## State

The daemon stores its local state under `CODEX_HOME/app-server-daemon/`:

- `settings.json` for persisted launch settings
- `app-server.pid` for the app-server process record
- `app-server-updater.pid` for the pid-backed standalone updater loop
- `daemon.lock` for daemon-wide lifecycle serialization
