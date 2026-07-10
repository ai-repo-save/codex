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

`bootstrap` requires the standalone managed install. It records daemon settings
under `CODEX_HOME/app-server-daemon/`, starts app-server as a pidfile-backed
detached process, and reads `app_server_auto_update` from
`CODEX_HOME/config.toml` to decide whether the detached updater loop should run.
Unset config enables the updater.

Set this user config before running `bootstrap` to keep the daemon on the
currently installed managed binary:

```toml
app_server_auto_update = false
```

With that setting, `bootstrap` stops any existing daemon updater loop and does
not launch a new one. The daemon still starts and restarts app-server from
`CODEX_HOME/packages/standalone/current/codex`; that file changes only when the
user or another package manager updates it explicitly.

## Installation and update cases

The daemon assumes Codex is installed through `install.sh` and always launches
the standalone managed binary under `CODEX_HOME`.

| Situation | What starts | Does this daemon fetch new binaries? | Does a running app-server eventually move to a newer binary on its own? |
| --- | --- | --- | --- |
| `install.sh` has run, but only `start` is used | `start` uses `CODEX_HOME/packages/standalone/current/codex` | No | No. The managed path is used when starting or restarting, but no updater is installed. |
| `install.sh` has run, then `bootstrap` is used with default config | The pidfile backend uses `CODEX_HOME/packages/standalone/current/codex` | Yes. Bootstrap launches a detached updater loop that runs `install.sh` hourly. | Yes, while that updater process is alive and app-server is already running. After a successful fetch, the updater restarts app-server with the refreshed binary and only then replaces its own process image. |
| `install.sh` has run, then `bootstrap` is used with `app_server_auto_update = false` | The pidfile backend uses `CODEX_HOME/packages/standalone/current/codex` | No. Bootstrap stops any existing updater loop and does not run `install.sh`. | No daemon updater refresh occurs. A future explicit `restart` still uses whatever binary is present at the managed path. |
| Some other tool updates the managed binary path | The next fresh start or restart uses the updated file at that path | Only when bootstrap starts the updater, because the updater runs `install.sh` on its normal cadence. | Without a bootstrap-managed updater loop, no. With one, the next successful updater pass compares the managed binary contents after `install.sh` runs; if app-server is running and they differ from the updater's current image, it refreshes app-server first and then itself. |

### Standalone installs

For installs created by `install.sh`:

- lifecycle commands always use the standalone managed binary path
- `bootstrap` is supported
- `bootstrap` starts a detached pid-backed updater loop that fetches via
  `install.sh` when `app_server_auto_update` is unset or `true`
- `bootstrap` stops the updater loop and does not fetch via `install.sh` when
  `app_server_auto_update = false`
- after a successful refresh, if app-server is running and the managed binary
  contents changed, the updater restarts app-server with that binary first and
  only then replaces its own process image
- the updater loop is not reboot-persistent; it must be started again by
  rerunning `bootstrap` after a reboot

### Out-of-band updates

This daemon does not watch arbitrary executable files for replacement. If some
other tool updates the managed binary path:

- without `bootstrap`, a currently running app-server remains on the old
  executable image until an explicit `restart`
- with a bootstrap-managed updater loop, the detached updater loop notices the
  changed managed binary on its next successful scheduled pass after running
  `install.sh`; if app-server is running, it refreshes app-server first and then
  refreshes itself once that replacement starts successfully

## Lifecycle semantics

`start` is idempotent and returns after app-server is ready to answer the normal
JSON-RPC initialize handshake on the Unix control socket.

`restart` stops any managed daemon and starts it again.

`enable-remote-control` and `disable-remote-control` persist the launch setting
for future starts. If a managed app-server is already running, they restart it
so the new setting takes effect immediately.

Top-level `codex remote-control` bootstraps with `--remote-control` when the
daemon is not already bootstrapped. With auto-update enabled, the updater loop
is the bootstrap marker. With `app_server_auto_update = false`, the managed
app-server process is the bootstrap marker because no updater loop is expected
to run. Otherwise it enables remote control and starts the daemon normally.

`stop` sends a graceful termination request first, then sends a second
termination signal after the grace window if the process is still alive.

All mutating lifecycle commands are serialized per `CODEX_HOME`, so a concurrent
`start`, `restart`, `enable-remote-control`, `disable-remote-control`, `stop`,
or `bootstrap` does not race another in-flight lifecycle operation.

## State

The daemon stores its local state under `CODEX_HOME/app-server-daemon/`:

- `settings.json` for persisted launch settings, including remote-control and
  auto-update mode
- `app-server.pid` for the app-server process record
- `app-server-updater.pid` for the pid-backed standalone updater loop
- `daemon.lock` for daemon-wide lifecycle serialization
