# Architecture

PasteForward is a single Rust binary with a small module boundary:

- `config`: JSON config, config/state paths, validation
- `clipboard`: local image clipboard readers
- `doctor`: remote detection and allowlisted command construction
- `daemon`: one process that fans out image changes to all destinations
- `history`: metadata JSONL and optional image cache
- `service`: launchd/systemd user service installation
- `command`: process and SSH execution helpers
- CLI `ssh`: optional preflight wrapper for remote agent sessions

## Runtime Model

There is one local daemon per user:

```text
local image clipboard
        |
        v
pasteforward daemon
        |
        +-- ssh destination A -> remote clipboard
        +-- ssh destination B -> remote clipboard
        +-- ssh destination C -> remote clipboard
```

Adding or deleting a destination updates config and restarts the service if it is
installed. The daemon also reloads config on every poll loop, so config changes
are picked up without a schema migration step.

Plain SSH sessions work after the daemon is running; the remote terminal agent
does not need to be launched through PasteForward.

The `pasteforward ssh <dest> -- claude` and `pasteforward ssh <dest> -- codex`
commands are convenience wrappers. They do not create per-destination daemons.
They validate the selected destination, ensure the local service is available
when explicitly allowed, run doctor checks, and then attach an SSH TTY to the
configured host.

## Remote Cache

The default remote cache is:

```text
/tmp/pasteforward
```

Remote filenames include the destination name, timestamp, and image hash prefix.

Cleanup uses local transfer metadata and removes only paths under the configured
remote cache prefix.
