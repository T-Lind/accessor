# Local access lock

Configure a passphrase with `/password` in the dashboard, Settings → Security,
or `acc password set` in a terminal. The application does not choose your real
passphrase for you. Use four unrelated, easy-to-recognize words. Enrollment
requires at least three words and 12 characters. Entry is masked in the dashboard
and the dedicated CLI prompt. A plain terminal can echo input; use the dashboard
or dedicated CLI when entering secrets by keyboard.

After setting a password, Accessor locks immediately. Every subsequent launch
starts locked. Say **“29 unlock [your passphrase]”** in one utterance, then say
“29” and your request. Substitute your configured wake code. Alternatively,
type `/unlock`, then enter the passphrase in the masked field. Saying **“29 lock”**
or typing `/lock` locks again; while awake, “lock” alone also works. During audible
output, the existing wake-interruption limitations still apply; keyboard `/lock`
is the reliable fallback.

Spoken unlocking is enabled by default. It uses local transcription, with cloud
STT/streaming disabled while locked. Unlock phrases never enter agent history,
relevance checks, activity messages, or analytics. Case, punctuation and whitespace
are normalized, but every word must match exactly and in order. There is no fuzzy
password matching or speaker identification. Choose words rather than digits to
avoid differences such as “42” versus “forty two.” Speak the secret only while
locked: after an ordinary wake, opted-in cloud streaming uploads speech before
its words are known. Canary/Parakeet process audio in memory; the optional external
Whisper CLI uses temporary WAV files that are deleted after decoding.

The password is a shared secret: someone who hears or records it can repeat it.
For keyboard-only access, turn off **Security → Spoken unlock**, or run:

```sh
acc config set security.spoken-unlock false
acc config set security.lock-seconds 3600
```

Auto-lock is an **absolute deadline since the last successful unlock**, default
one hour. It is separate from the conversation's idle sleep timer. Speech, noise,
agent work and playback cannot extend it. The setting accepts 1–86400 seconds;
zero is rejected. Security preferences are not exposed through agent-editable MCP
settings. Local dashboard changes apply immediately; CLI configuration is loaded
on the next launch.

Locking revokes normal voice, keyboard, settings and live MCP control access;
stops main/worker processes, playback, synthesis and compaction; invalidates queued
voice; and clears the dashboard and in-memory audio cache. Scheduled work, alarms
and incoming events wait while locked. Already claimed jobs are marked interrupted
instead of replayed. Work that becomes due while locked can run after unlocking.
Actions already performed by an external service cannot be undone by locking.

There is one live Accessor interface per settings directory. File locking also
prevents a second interface from using stale password state or a separate guess
counter. Use `/password` to change a running session's password. The CLI requires
the running interface to close first. Changing/removing an existing password
through the CLI requires the current passphrase. In the dashboard, unlock first.

Passwords are stored only as salted **Argon2id** hashes in `password.json` beside
the settings (19 MiB memory, two iterations, one lane). Failed attempts receive
increasing cooldowns, persisted across restarts. Malformed or unreadable password
files fail closed. Back up the hash file with settings if migrating this profile.

This protects an unattended **Accessor interface**, not an unlocked OS account.
Someone who can edit its files, change `ACC_HOME`, run a native agent CLI, or use
the same account's standalone MCP/management tools can bypass this application
boundary. Use an OS password and disk encryption for that threat. Forgotten
passphrases can be reset by the OS owner by removing `password.json` while Accessor
is stopped. The lock does not delete provider transcripts or erase terminal
scrollback. A native agent terminal is outside this lock, so the in-app shortcut
is disabled when a password is configured.

Spoken-reply caching now stays in bounded RAM (32 short clips / 8 MiB) and clears
on locking or exit. Previously generated disk caches are no longer read. To remove
their generated WAV files, run `acc tts clear-cache`; this leaves unrelated files
alone. Exporting a test with `--output` still explicitly writes the requested WAV.

Regression coverage: `python tests/security_runtime.py` exercises protected
commands, exact spoken/typed unlock, restarts, cooldown persistence, auto-lock
during work, MCP refusal and corrupt-password startup. Physical room replay,
microphone accuracy and OS account hardening are separate deployment tests.
