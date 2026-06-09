# Onboarding

PasteForward is destination-oriented. A destination is one SSH target that should
receive local image clipboard updates.

## Add A macOS Destination

```sh
pasteforward init macmini --host acartagena@arnolds-mac-mini.tail46d819.ts.net
```

Expected flow:

1. config is written to `~/.config/pasteforward/config.json`
2. local clipboard backend is checked
3. SSH connectivity is checked
4. remote clipboard commands are checked
5. remote cache dir is created under `/tmp/pasteforward`
6. PasteForward asks to install/restart the local service

## Add A Linux GUI Destination

Wayland:

```sh
pasteforward init devbox \
  --host user@devbox \
  --remote-mode linux-wayland \
  --remote-env WAYLAND_DISPLAY=wayland-0 \
  --remote-env XDG_RUNTIME_DIR=/run/user/1000
```

X11:

```sh
pasteforward init devbox \
  --host user@devbox \
  --remote-mode linux-x11 \
  --remote-env DISPLAY=:0
```

PasteForward does not install `wl-copy` or `xclip`. `doctor` reports missing
tools and leaves package installation to the operator.

## Check Health

```sh
pasteforward doctor
pasteforward status
pasteforward list
```

## Start Claude Or Codex

```sh
pasteforward ssh macmini -- claude
pasteforward ssh macmini -- codex
```

For non-interactive setup, make service installation explicit:

```sh
pasteforward init macmini --host acartagena@arnolds-mac-mini.tail46d819.ts.net --yes
```

## Remove A Destination

Keep metadata history:

```sh
pasteforward delete devbox
```

Remove destination history too:

```sh
pasteforward delete devbox --purge
```

If the last destination is deleted, PasteForward uninstalls the local service.
