# Architecture

Accessor is a long-lived local process that sits between a microphone and one or
more command-line agent harnesses. It owns the latency-sensitive path (capture,
wake, speech recognition, playback) and the durable local state (settings, shared
memory, the organizer, notifications, the encrypted journal). Each harness keeps
its own tools, sandbox, and native conversation thread.

The diagrams below render on this site and on GitHub. The editable source of the
pipeline diagram is [`docs/diagrams/29.dot`](https://github.com/T-Lind/accessor/blob/main/docs/diagrams/29.dot);
regenerate the PNG with Graphviz `dot`.

## End-to-end voice pipeline

![Accessor 29 core voice pipeline](diagrams/29.png)

## Component map

The microphone path is local and bounded; the agent path is asynchronous. Nothing
on the capture path waits on a network call.

```mermaid
flowchart LR
  mic["Microphone"] --> dsp["Downmix, resample 16 kHz,<br/>AEC3 echo cancel"]
  dsp --> vad["Earshot VAD"]
  vad --> wake["Local wake detector<br/>('29')"]
  wake --> stt["Local STT<br/>Canary / Parakeet / Whisper"]
  stt --> gate["Relevance + input gate"]
  gate --> router["Routing"]
  router --> main["Main harness"]
  main --> worker["Coding / plugin worker<br/>(isolated, 15 min)"]
  worker -. "bounded result" .-> main
  main --> ttsq["Ordered speech queue"]
  ttsq --> spk["Speakers"]
  spk -. "echo reference" .-> dsp

  main <--> mem[("Shared memory")]
  main <--> mcp["Accessor MCP tools"]
  mcp <--> org[("Notes, alarms,<br/>tasks, notifications")]
  mcp <--> live["Live session control"]
```

## Routing and delegation

Relevance and routing decide whether speech is addressed to the agent, then pick
the main, coding, or plugin role. With the coordinator policy (on by default) a
lightweight main agent owns the conversation and delegates from its metaprompt;
keyword/state routing is the optional legacy path.

```mermaid
flowchart TD
  utt["Completed utterance"] --> rel{"Addressed<br/>to the agent?"}
  rel -->|no| drop["Ignored<br/>(kept out of history)"]
  rel -->|yes| coord["Main conversation"]
  coord -->|"coding or difficult work"| work["Isolated worker<br/>fresh process, own model, 15 min"]
  coord -->|"connector / plugin work"| plug["Plugin harness"]
  work --> res["Bounded result"]
  res --> coord
  plug --> coord
  coord --> sleep["Sleep after idle"]
```

## Memory and MCP

Shared memory is a local JSON store with file locking and atomic replacement.
A bounded, ranked digest is injected when a harness starts (not every turn), so
saving a fact does not restart a warm session or break provider prompt caching.
Accessor's MCP server exposes memory, notes, the organizer, session controls,
harness health, and subscription usage over stdio.

```mermaid
flowchart LR
  convo["Conversation"] --> capture["Off-hot-path capture"]
  capture --> store[("memory.json")]
  store --> rank["Rank by importance,<br/>recency, access count"]
  rank --> digest["Bounded digest"]
  digest --> prompt["Harness launch instructions"]
  mcp["MCP memory tools"] <--> store
  notes[("notes/")] <--> mcp
  sched[("schedules.json")] <--> mcp
  notif[("notifications.json")] <--> mcp
```

## Wake, sleep, and lock states

Sleep ends active listening but leaves the local wake detector on. The password
lock is separate and blocks both voice and typed access. Auto-lock is an absolute
deadline since the last successful unlock.

```mermaid
stateDiagram-v2
  [*] --> Asleep
  Asleep --> Awake: 29 plus chime
  Awake --> Thinking: request
  Thinking --> Awake: reply spoken or work done
  Awake --> Asleep: idle timeout or go to sleep
  Asleep --> Locked: auto-lock deadline
  Awake --> Locked: lock command
  Locked --> Asleep: unlock passphrase
  Locked --> Awake: 29 then request
```

See [Local access lock](SECURITY.md) for the threat model and recovery steps.

## Incoming events

The only supported event source is the local `acc events emit` handoff. Accessor
opens no HTTP listener and stores no provider credentials.

```mermaid
sequenceDiagram
  participant Mail as Trusted email automation
  participant Acc as acc events
  participant Q as Local event queue
  participant A as Harness
  Mail->>Acc: acc events emit --thread-id ... --message-id ...
  Acc->>Q: validated metadata (IDs only)
  Q->>A: idle consumer starts a turn
  A-->>Q: receipt on completion
  Note over A: replies through the agent's own<br/>Gmail connector, not Accessor
```

## Watches and notifications

A watch is a recurring, gated survey. Each run is an isolated worker that may
emit only `notify` findings; each finding is scored and deduplicated before it
reaches you.

```mermaid
flowchart LR
  watch["Watch / scheduled task"] --> run["Isolated run"]
  run --> score{"Score vs threshold"}
  score -->|below| drop["Dropped"]
  score -->|pass| dedupe{"Seen recently?"}
  dedupe -->|yes| drop
  dedupe -->|no| store[("notifications.json")]
  store --> ui["Activity notice + ding + optional speech"]
```

## Where the work runs

| Stage | Where | Hot path? |
| --- | --- | --- |
| Capture, resample, AEC3 | Rust audio thread | Yes |
| VAD and wake detection | Rust, bounded rolling windows | Yes |
| Local STT | Rust / ONNX on completed clips | Off the UI loop |
| Cloud STT (opt-in) | WebSocket upload while awake | Off the wake path |
| Routing and relevance | Rust, optional Jev classifier | Off capture |
| Harness processes | Child processes under a job object / process group | Async |
| Memory, organizer, triggers | File-locked local stores | Moved off the event loop |
| Synthesis and playback | Ordered queue with echo reference | Feeds AEC |

## Design rules

- **Keep the hot path clear.** Wake detection, STT, and playback are
  latency-sensitive. New work (network, disk, model loads, subprocesses) belongs
  off that path, and overload is reported visibly.
- **Local by default.** Cloud transcription and speech are opt-in, and credentials
  live in the OS credential store, never in `config.json`.
- **Bounded everything.** Queues, rolling windows, memory, notifications, and
  worker output are capped. Agent-authored data is never authority.
- **Fail closed.** Malformed password files, unreadable journal keys, and
  unsupported approval requests are refused rather than guessed.

<script src="https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js"></script>
<script>
  document.addEventListener("DOMContentLoaded", function () {
    if (!window.mermaid) return;
    mermaid.initialize({ startOnLoad: false, theme: "neutral" });
    document
      .querySelectorAll("pre > code.language-mermaid")
      .forEach(function (code) {
        var host = document.createElement("div");
        host.className = "mermaid";
        host.textContent = code.textContent;
        code.parentElement.replaceWith(host);
      });
    mermaid.run({ nodes: document.querySelectorAll(".mermaid") });
  });
</script>
<style>
  .mermaid { max-width: 100%; overflow-x: auto; margin: 1.25rem 0; }
  .mermaid svg { max-width: 100%; height: auto; }
</style>
