# Presence briefs

Accessor can speak a short, local brief when you sit down. It is deliberately
not an agent turn: the camera detects an arrival, Accessor reads its own
notifications and organizer state, and speaks a couple of sentences. No model
call, no Gmail poll, and no frame leaves the machine.

The camera is optional. Anything that can run a local command can be an arrival
sensor: a PIR, a phone shortcut, a session-unlock hook, or `acc presence emit`
by hand.

## How it works

```
camera / PIR / manual
        │  metadata only: source, confidence
        ▼
acc presence emit
        ▼
~/.config/accessor/presence/pending/*.json
        ▼
Accessor gates: enabled · unlocked · idle · cooldown · quiet hours · confidence
        ▼
local brief: unread notifications + scheduled tasks + alarms
```

The sensor never supplies the spoken text. The event contains only a source and
a confidence, so a compromised or buggy sensor cannot inject a prompt or a
command. Events are regular private files; there is no HTTP listener.

## Enable and test

```sh
acc presence enable
acc presence status
acc presence emit --source manual          # simulate an arrival
```

Restart Accessor after enabling. The live process reads presence settings at
startup.

The brief is spoken while the session is asleep too, provided it is unlocked and
idle. It does not open the conversation; say “29” if you want to follow up.

Settings:

```sh
acc config set presence.enabled true
acc config set presence.cooldown-seconds 1800
acc config set presence.min-confidence 0.4
acc config set presence.quiet true
acc config set presence.quiet-start 22:00
acc config set presence.quiet-end 07:00
acc config set presence.greet-when-empty true
```

`min-confidence` filters motion-only arrivals; a confirmed person should send
0.8–0.95, while a motion fallback sends 0.5.

## Camera sensor

The `monovision` checkout includes a small camera sensor. It uses motion first
and a 29 MB SSD MobileNet person detector (COCO, Apache-2.0) only when there is
motion or every few seconds. It is not a depth model and it does not record
video.

```sh
cd ~/source/repos/monovision
.venv/bin/monovision presence --acc-bin "$(command -v acc)"
```

Useful flags:

- `--image photo.jpg` — test person detection on one image and exit.
- `--dry-run` — print the `acc presence emit` command instead of running it.
- `--once` — exit after the first arrival (useful for testing).
- `--no-person` — motion only, no model download.
- `--require-person` — ignore motion that is not confirmed by a person.
- `--motion-threshold 0.01` — fraction of the 320×240 frame that must change.
- `--person-threshold 0.55` — SSD confidence.
- `--away-seconds 90` — how long after the last motion/person the room is
  considered empty.
- `--debug-dir out/presence` — write annotated debug frames. Off by default.

The sensor runs at camera speed (about 8 FPS on the E4HD), uses two OpenCV DNN
threads for the detector, and stops emitting until it has seen the room empty and then
occupied again. Accessor's own cooldown is the second line of defence.

### systemd user service (optional)

```ini
# ~/.config/systemd/user/accessor-presence.service
[Unit]
Description=Monovision camera presence sensor
After=graphical-session.target

[Service]
WorkingDirectory=%h/source/repos/monovision
ExecStart=%h/source/repos/monovision/.venv/bin/monovision presence --acc-bin %h/.cargo/bin/acc
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now accessor-presence
journalctl --user -u accessor-presence -f
```

The service and Accessor must run as the same user so the sensor can write the
private queue and `acc` can find the same `ACC_HOME`.

## What the brief says

Examples:

- “Welcome back. You have three unread notifications and one scheduled task. The
  newest is Inbox needs a reply.”
- “Welcome back.”
- Nothing at all inside quiet hours, while locked, while Accessor is busy or
  speaking, or within the cooldown.

The brief reads counts, not full notification bodies. Ask Accessor to review
them if you want the details. This keeps the greeting short and avoids reading
private mail aloud to an empty room.

## Email without polling

Accessor does not poll Gmail and does not store OAuth credentials. Use the
harness's Gmail connector through a watch, which is already read-only unless you
ask for a reply:

```text
/watch every 2h threshold=0.6 email brief: unread messages from the last two
days that need a reply from me. Read-only; do not send, archive, or open links.
```

The watch raises a notification; the arrival brief counts it and reads its
title. Set `quiet` or `speak=false` on the watch if you want it to stay silent
until you ask. Watches run only while Accessor is running, so a laptop that was
closed overnight may need a fresh watch run after you start it.

## Privacy and limits

- No frames are stored by default. `--debug-dir` is explicit and local.
- The person detector runs locally; the model is downloaded once.
- Accessor never opens the camera. The sensor owns the device, so do not run it
  at the same time as `monovision live` on the same camera.
- The camera sees only what is in front of the screen. A closed lid means no
  camera presence at all; a PIR or a session-unlock hook is better for “entered
  the room.”
- A person detector can miss someone seated behind a monitor, and motion can
  fire on curtains, pets, or a lighting change. Use `--require-person`, tune the
  thresholds, or choose a PIR if you need fewer false positives.
- Not a security sensor. It does not identify people and is not suitable for
  intruder detection or safety decisions.
- The MCP surface does not expose a camera stream. Snapshot tools would be a
  separate, explicitly granted feature; presence is metadata only.
