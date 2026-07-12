# Contracts

PasteForward has one job: forward local image clipboard changes to configured
SSH destinations so terminal agents can paste images remotely.

## Config

Config path:

```text
~/.config/pasteforward/config.json
```

State path:

```text
~/.local/state/pasteforward/
```

Shape:

```json
{
  "version": 1,
  "remote_dir": "/tmp/pasteforward",
  "retention": {
    "ttl_seconds": 3600
  },
  "history": {
    "metadata": true,
    "image": false
  },
  "daemon": {
    "interval_millis": 1000
  },
  "destinations": {
    "macmini": {
      "host": "user@mac.example",
      "enabled": true,
      "remote_mode": "auto"
    }
  }
}
```

Destination names may contain only ASCII letters, digits, `-`, and `_`.

SSH hosts must be non-empty, must not begin with `-`, and must not contain
ASCII control characters. PasteForward passes the host to `ssh` as one
destination argument and does not accept embedded SSH options.

Remote environment overrides are limited to:

- `DISPLAY`
- `WAYLAND_DISPLAY`
- `XDG_RUNTIME_DIR`

Unknown keys, blank values, malformed config, and unsupported config versions
reject the entire config before any local or remote side effect.

Remote cache directories use one canonical lexical form: exactly one leading
`/`, one or more non-empty path segments, no trailing or repeated `/`, and no
`.` or `..` segment. Root spellings such as `/`, `//`, and `///` are rejected.
The same validation runs again at every exported execution boundary.

## Initialization Contract

`pasteforward init` without arguments starts an interactive setup wizard.
`pasteforward init <dest> --host <ssh-host>` remains the non-interactive shape.

Initialization builds and validates a candidate config in memory, then runs
local and remote doctor checks. The config is committed only after all required
checks pass. A failed check returns a non-zero exit status and does not write
config, install a service, or create the remote cache directory.

After checks pass, initialization creates the remote cache, commits config, and
installs or restarts the user service only after explicit interactive consent,
`--yes`, or `--install-service`.

`doctor` is read-only. It returns non-zero when the local clipboard or any
selected destination fails a required check.

## End-To-End Test Contract

`pasteforward test <dest>` reads the current local clipboard image, forwards it
once to the selected destination, reads the destination clipboard image back,
and compares SHA-256 hashes. It reports success only when they match.

The test has fixed command timeouts and byte caps. It does not start the daemon,
write transfer history, or retain image bytes locally. It intentionally replaces
the selected remote clipboard and says so before running interactively.

## Remote Modes

Supported remote modes:

- `auto`
- `macos-pasteboard`
- `linux-wayland`
- `linux-x11`

`auto` detects the remote OS, installed clipboard tools, and a reachable GUI
session over SSH. Wayland is selected only when its socket is usable; otherwise
PasteForward falls back to a reachable X11 socket.

For `macos-pasteboard`, PasteForward writes the remote temp PNG to the
pasteboard as `public.png`, adds `public.tiff` when AppKit can render it, and
adds a `public.file-url` reference to the same remote temp file.

## Daemon Contract

The daemon:

- reads config every poll loop
- watches local image clipboard changes only
- forwards a new image hash to all enabled destinations
- reads the remote clipboard back and records transfer metadata only after the
  bytes match the source SHA-256
- periodically removes expired remote files from paths it previously wrote

The daemon does not sync clipboard text.

Only one daemon should run for a user. Startup refuses to replace a live daemon
pid and overwrites stale pid files.

Service definitions use a stable absolute executable path that is a regular,
executable file. PATH resolution skips non-executable shadow files and selects
the same executable entry a shell can run. Package upgrades must not leave
launchd or systemd pointing at a removed versioned path.

`install-service` hands an existing recorded daemon over to the platform service
manager before activation. Reinstalling a systemd unit repairs `failed` units
as stopped units. If activation fails, the previous service file, persistent or
runtime enablement, and active or stopped state are restored; cleanup and daemon
reload are both attempted on every rollback path. An independently running
manual daemon that was stopped for the handoff is restarted if activation fails.
Restoration reports success only after the daemon publishes a PID-bound ready
marker following local-backend and config initialization and remains healthy for
a bounded stability window; platform-service activation uses the same readiness
bar. Failed or timed-out manual starts are terminated and reaped.

`install-service` and `uninstall-service` change only the local user service.
They never add, delete, or purge destinations or history.

## History Contract

Metadata history is JSONL:

```text
~/.local/state/pasteforward/history.jsonl
```

Each transfer records:

- destination
- host
- SHA-256
- byte size
- remote path
- remote mode
- optional local image-history path

Image bytes are never stored locally unless image history is explicitly enabled.
