# Agent Usage

PasteForward is meant to be boring to use from terminal agents.

## Start A Remote Agent

After a destination exists and the local service is installed, use the same SSH
flow you already use:

```sh
ssh user@mac.example
claude
# or: codex
```

PasteForward forwards image clipboard changes in the background. Claude Code,
Codex, or any other terminal agent keeps using its normal paste path on the
remote machine.

## Non-Interactive Agent Setup

Use explicit service consent in non-interactive sessions:

```sh
pasteforward init macmini \
  --host user@mac.example \
  --yes
```

Then run your normal SSH session:

```sh
ssh user@mac.example
claude
```

## Multiple Destinations

There is one local daemon and one config file. The daemon reloads config every
poll loop and forwards each new local image clipboard hash to every enabled
destination.

Add another destination:

```sh
pasteforward init linuxvm --host user@linuxvm.example --yes
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
