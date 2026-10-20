# Music on this machine

Accessor can play local audio through MPV or control Spotify through a separately
running Soloist player. An enabled Spotify plugin in Codex and a paired local
player are separate connections: discovering a track does not start a player on
your computer.

| What you want | What must be available |
| --- | --- |
| Local files or generated ambience | MPV on Linux or macOS and a running Accessor session |
| Spotify through `/music spotify` or Accessor's MCP | Soloist on this Linux machine, its own API key, account pairing, and local controls enabled |
| Spotify discovery through Codex | The Spotify plugin connected in the selected Codex harness |
| Spotify in a browser or desktop app | That player signed in on this machine; Accessor's Soloist controls do not control it |

## Local files and ambience

Install [MPV](https://mpv.io/installation/). Accessor checks `ACC_MPV_BIN`, then
`music/bin/mpv` inside its configured assets folder, then `PATH`. Find that folder
with `acc config locations`. Music players are separate prerequisites;
Accessor's speech-model setup does not install them.

Type these commands inside the running Accessor interface:

```text
/music play /home/me/Music/a song.mp3
/music ambience brown
/music volume 20
/music pause
/music resume
/music status
/music stop
```

Use absolute paths, without shell quotes, in the interface. Supported files are
MP3, WAV, FLAC, OGG, M4A, AAC, and Opus. Ambience also accepts `rain` and `white`;
these are generated noise textures. No downloader is included.

An agent launched by Accessor can use the session-bound `music_control` MCP tool
or `acc music play '/home/me/Music/a song.mp3' --repeat --volume 20`. A standalone
Codex session needs Accessor's MCP connection and live session capability to use
that tool. An unrelated terminal needs `acc music --session SESSION_FILE ...`;
the endpoint file is private and must not be shared or committed.

## Set up Spotify Soloist on Linux

Soloist plays through this computer's audio output. It requires Spotify Premium,
a personal Soloist API key, and initial pairing from a Spotify app on the same
local network. Generate the key at the [Soloist dashboard](https://developer.spotify.com/dashboard/soloist)
and complete any account or terms prompts yourself. Follow Spotify's
[getting-started guide](https://developer.spotify.com/documentation/soloist/tutorials/getting-started)
for the official download matching your architecture. Accessor does not ship
Soloist or create its account credentials.

Put the executable on `PATH`, set `ACC_SOLOIST_BIN` in the environment that
launches Accessor, or install it at `music/bin/soloist` inside Accessor's assets
folder. Below, `soloist` means that executable; substitute its full path when it
is not on `PATH`.

For a first run, use this **Bash** snippet in a separate terminal. It reads the
key without displaying it or putting the literal key in shell history, starts
the player, and clears the shell variable when the player exits:

```bash
(
  set +x
  umask 077
  read -r -s -p 'Soloist API key: ' soloist_key
  printf '\n'
  if [ -z "$soloist_key" ]; then
    printf 'No key entered.\n' >&2
    exit 1
  fi
  trap 'unset soloist_key' EXIT
  soloist --device-name 'Accessor on this computer' \
    --api-key "$soloist_key" --initial-volume 20 \
    --cache-size 512 --ws 127.0.0.1:0
)
```

Soloist's CLI receives the key as a process argument. Do not share process
listings, logs, screenshots, or crash reports containing it. For persistent
startup, retrieve the key from an OS credential store or your secret manager;
never put it in Accessor's `config.json`, a checked-in script, or a service unit.
The snippet does not save the API key or configure automatic startup.

Keep this terminal running. In the Spotify app's device picker, select
**Accessor on this computer** and start a track. Selecting a phone or another
speaker instead sends playback there. Pairing is stored locally for subsequent
runs; the API key is still needed at startup.

Use the same OS user and data-directory environment for Soloist and Accessor.
Soloist normally uses `~/.local/share/soloist`, but `STATE_DIRECTORY` and
`XDG_DATA_HOME` can change that location. Accessor invokes `soloist ctl` without
a custom data directory, so starting the daemon with a different `--data-dir`
alone makes it invisible to Accessor. See the [daemon CLI reference](https://developer.spotify.com/documentation/soloist/reference/command-line).

Bind the control API to loopback as above. It has no built-in client
authentication; do not expose it on `0.0.0.0` or forward its port. A random port
is discovered through Soloist's runtime files. See the
[WebSocket reference](https://developer.spotify.com/documentation/soloist/reference/websocket-api).

## Verify and use Spotify from Codex

First check the player directly:

```sh
soloist ctl status
soloist ctl now --json
```

Status can succeed before account login. Check the authentication and playback
state, then confirm audio is coming from this computer. `now --json` observes
actual playback; a command receipt alone does not prove that a track is audible.
See the [control CLI reference](https://developer.spotify.com/documentation/soloist/reference/soloist-ctl).

Then type inside Accessor:

```text
/music spotify status
/music spotify volume 20
/music spotify resume
/music spotify pause
/music spotify next
/music spotify previous
/music spotify stop
```

To select content, use `/music spotify play spotify:track:TRACK_ID`; album,
playlist, and episode URIs also work. Replace the ID with its 22-character
Spotify ID. A web link such as `https://open.spotify.com/track/TRACK_ID?si=...`
becomes `spotify:track:TRACK_ID`; names and web URLs are not accepted by this
control. Ask Codex to find the track through its Spotify plugin and play its URI
through Accessor's `music_control` tool with `backend: "spotify"`. The tool must
be available in that Accessor-launched session.

If the plugin was just connected, refresh Accessor's connector inventory with
`acc connectors refresh` and restart Accessor. Codex plugins can include skills
and service connections; installing one does not install a local audio player.
See [Harnesses](harnesses.md#connectors-are-reused) and the
[official plugin documentation](https://learn.chatgpt.com/docs/plugins).

## Playback lifecycle and common failures

Local music defaults to 25% volume and drops to one fifth during Accessor's
spoken replies and while user speech is captured or processed. Cancellation, `/stop`, locking, and exit stop the owned MPV
player. Sleep alone can leave background music playing. Spotify does not
automatically duck and has its own lifecycle: `/music spotify stop` pauses it;
Accessor cancellation or exit does not terminate Soloist. Stop its foreground
process with Ctrl-C when finished. Both players output outside Accessor's speech
echo reference, so headphones or moderate speaker volume improve voice control.

- **Soloist is not running / no PID file:** start it with the key and `--ws`;
  an installed binary or enabled plugin is not a running daemon.
- **Running but not logged in:** finish pairing in Spotify's device picker.
- **Controls cannot connect:** restart Soloist with `--ws 127.0.0.1:0` and check
  that Accessor and Soloist resolve the same data directory.
- **Device missing from Spotify:** check local network discovery, VPN, firewall,
  and wireless client isolation. Do not expose the control API to fix discovery.
- **Playing but silent here:** check the selected Spotify device, system output,
  mute, and volume. On Linux, `wpctl status` shows active output routing.
- **Expired build (daemon exit 10):** obtain a current official Soloist build.
- **MPV cannot begin playback:** check the file, executable, and system output.
- **Music tool unavailable in standalone Codex:** use an Accessor-launched
  session, or configure its MCP connection and private live session endpoint.
