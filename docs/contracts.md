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
      "host": "acartagena@arnolds-mac-mini.tail46d819.ts.net",
      "enabled": true,
      "remote_mode": "auto"
    }
  }
}
```

Destination names may contain only ASCII letters, digits, `-`, and `_`.

## Remote Modes

Supported remote modes:

- `auto`
- `macos-pasteboard`
- `linux-wayland`
- `linux-x11`

`auto` detects the remote OS and available clipboard tool over SSH.

For `macos-pasteboard`, PasteForward writes the remote temp PNG to the
pasteboard as `public.png`, adds `public.tiff` when AppKit can render it, and
adds a `public.file-url` reference to the same remote temp file.

## Daemon Contract

The daemon:

- reads config every poll loop
- watches local image clipboard changes only
- forwards a new image hash to all enabled destinations
- records transfer metadata when history is enabled
- periodically removes expired remote files from paths it previously wrote

The daemon does not sync clipboard text.

Only one daemon should run for a user. Startup refuses to replace a live daemon
pid and overwrites stale pid files.

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
