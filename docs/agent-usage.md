# Agent Usage

PasteForward is meant to be boring to use from terminal agents.

## Start A Remote Agent

After a destination exists and the local service is installed, use the same SSH
flow you already use:

```sh
ssh acartagena@arnolds-mac-mini.tail46d819.ts.net
claude
# or: codex
```

PasteForward forwards image clipboard changes in the background. Claude Code,
Codex, or any other terminal agent keeps using its normal paste path on the
remote machine.

## Optional SSH Wrapper

The wrapper is useful when an agent or script wants PasteForward to do preflight
checks before opening SSH:

```sh
pasteforward ssh macmini -- claude
pasteforward ssh macmini -- codex
```

The wrapper:

- validates the destination
- checks the local clipboard backend
- installs or restarts the background service only after confirmation or an
  explicit flag
- runs `doctor` checks for the destination
- opens SSH with a TTY
- executes the requested command through the remote login shell

The wrapper is not required for image forwarding. Once the daemon is running,
plain `ssh` sessions work.

## Non-Interactive Agent Setup

Use explicit service consent in non-interactive sessions:

```sh
pasteforward init macmini \
  --host acartagena@arnolds-mac-mini.tail46d819.ts.net \
  --yes
```

Then run your normal SSH session, or use the wrapper if you want preflight
checks:

```sh
ssh acartagena@arnolds-mac-mini.tail46d819.ts.net
claude
```

```sh
pasteforward ssh macmini -- claude
```

If the service is not installed and stdin is not interactive,
`pasteforward ssh` does not silently install it. Use:

```sh
pasteforward ssh macmini --install-service -- claude
```

## Multiple Destinations

There is one local daemon and one config file. The daemon reloads config every
poll loop and forwards each new local image clipboard hash to every enabled
destination.

Add another destination:

```sh
pasteforward init linuxvm --host user@linuxvm --yes
```

Check what the daemon will use:

```sh
pasteforward list
pasteforward status
pasteforward doctor
```

## Troubleshooting

Use `doctor` first:

```sh
pasteforward doctor macmini
```

For Linux GUI remotes, pass the GUI environment explicitly when SSH does not
inherit it:

```sh
pasteforward init devbox \
  --host user@devbox \
  --remote-mode linux-x11 \
  --remote-env DISPLAY=:0
```

The daemon only watches image clipboard changes. Text clipboard changes are
ignored.
