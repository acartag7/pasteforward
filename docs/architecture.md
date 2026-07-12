# Architecture

PasteForward is a single Rust binary with a small module boundary:

- `config`: JSON config, config/state paths, validation
- `clipboard`: local image clipboard readers
- `doctor`: remote detection and allowlisted command construction
- `remote`: remote mode resolution and clipboard command construction
- `daemon`: one process that fans out image changes to all destinations
- `history`: metadata JSONL and optional image cache
- `service`: launchd/systemd user service installation
- `command`: process and SSH execution helpers

## Setup Model

Initialization is a staged operation:

```text
CLI or wizard input
        |
        v
parse into typed candidate config
        |
        v
validate complete config (no side effects)
        |
        v
read-only local + SSH doctor probes
        |
        v
prepare remote cache -> atomically save config -> optional user service
```

The probe phase and prepare phase are separate so a failed doctor cannot leave
local config or a remote directory behind. CLI input and config loaded by the
daemon use the same validation functions. Public sync functions revalidate raw
config and destination objects before constructing or executing a command.

`pasteforward test <dest>` is a one-shot path outside the daemon. It uses the
same clipboard and SSH adapters, applies byte/time bounds, and verifies remote
clipboard readback against the local image hash without writing history.

## Runtime Model

There is one local daemon per user:

```text
local image clipboard
        |
        v
pasteforward daemon
        |
        +-- ssh destination A -> write -> bounded readback -> history
        +-- ssh destination B -> write -> bounded readback -> history
        +-- ssh destination C -> write -> bounded readback -> history
```

Adding or deleting a destination updates config and restarts the service if it is
installed. The daemon also reloads config on every poll loop, so config changes
are picked up without a schema migration step.

The service adapter records a stable absolute executable path. It preserves the
PATH-resolved symlink used to invoke the CLI instead of canonicalizing a
package-manager version directory.

Plain SSH sessions work after the daemon is running; the remote terminal agent
does not need to be launched through PasteForward.

## Remote Cache

The default remote cache is:

```text
/tmp/pasteforward
```

Remote filenames include the destination name, timestamp, and image hash prefix.

Cleanup uses local transfer metadata and removes only paths under the configured
remote cache prefix.
