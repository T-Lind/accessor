# Hands-free tasks and everyday conversation

Long-running research, coding and other multi-step work use the same task controls.
The main conversation can remain available while one separate worker runs.
A wake code pauses speech and opens listening; it preserves both main-agent work
and worker work. Explicit cancellation, lock, shutdown or the deadline can stop
work. Other requests made
while the main agent is busy queue for its next turn.

| Say while awake | Result |
| --- | --- |
| “stop talking” | Silence output; work continues |
| “task status” | Immediate local running/waiting status |
| “catch me up” | Latest local task progress or completed result |
| “tell the task keep the API unchanged” | Send guidance to the active worker, or main task when there is no worker |
| “continue speaking” | Resume the interrupted synthesis chunk; previously completed chunks stay completed |
| “cancel the task” | Cancel active main/worker work and playback |
| “stop, do this instead” | Explicitly cancel and replace the current request |

During audible output, say your wake phrase first, pause, then give the control.
With `wake-attention=true`, use “hey twenty-nine”, “hi twenty-nine”, or
“okay twenty-nine”; the bare number no longer activates the session or a playback
probe. An already open conversation still accepts ordinary follow-ups.

Speech continuation tracks completed synthesis chunks, not an estimated word
position. The interrupted chunk repeats. Streaming Cartesia and direct system
speech may use an entire reply as one chunk. `/repeat` replays the whole last
answer and can run while a worker is working.

## Drafting a longer request

Say “let me explain”, then speak in several segments. Pauses do not send the draft
to the agent. “Wait” holds it, “scratch that” removes the last segment, “discard the
draft” clears it, and “send that” submits the accumulated request once. Drafts
stay in memory, are bounded to 32,000 bytes/100 segments, and clear on lock. An
open draft holds the listening window open; the independent password timeout
still applies. Dictation/journaling remains a separate feature.

## Saved task records

```text
/task new Research local-first voice assistants using primary sources
/task new --isolated Fix the failing parser tests
/jobs
/steer Preserve public configuration compatibility
/task show TASK_ID
/task patch TASK_ID
/task cancel TASK_ID
/task resume TASK_ID Inspect the changes and continue with validation
/capabilities
```

Task IDs are shown on screen; eight leading characters are enough when unique.
The initial objective supplies the task's display name. Each worker saves a
private receipt under `ACC_HOME/jobs/`: objective, accepted guidance, harness,
model/reasoning, original and execution workspace, native thread ID where
supported, last progress, bounded result, pending-request descriptions and state.
Disk writes use a bounded background writer with atomic replacement. Overload and
write failures are reported.

`acc jobs list`, `acc jobs show ID`, and `acc jobs patch ID` inspect these records
from another terminal. The patch command shows the Git diff **stat** against HEAD;
use the saved workspace with native Git/editor tools for the full patch and
untracked files. Receipts with a last-recorded running/waiting state are labelled
unverified: a disk receipt cannot prove a process survived a restart. The active
`/jobs` status is the live view.

Resume is always explicit and requires the original canonical project workspace.
Codex workers use a persisted native thread and `thread/resume`; unsupported
harnesses start a fresh process with a bounded handoff. The continuation instructs
the agent to inspect existing files and external effects first. It does not
restore pending approvals, replay uncertain actions, undo files, or automatically
retry an interrupted job. A previously read-only task stays read-only on resume.

The default task deadline is **one hour**. Set `jobs.timeout-seconds` to 60–86400:

```sh
acc config set jobs.timeout-seconds 10800
```

`--isolated` creates a detached Git worktree from committed HEAD under
`ACC_HOME/worktrees/`. Uncommitted source changes remain in the original checkout
and are not copied. Worktrees are retained for inspection and explicit resume;
Accessor never automatically merges or deletes them. Normal delegation uses a
separate worker process in the existing workspace. Worktree preparation runs
outside the speech/UI loop.

## Questions and permissions without typing commands

Run `/approvals`, or `acc approvals` from another terminal, to open a local browser
panel. Its labelled forms and buttons work with a keyboard, screen reader or
keyboard-emulating assistive switch. Permission requests show their exact backend
details; question forms preserve question IDs and options. Requests expire after
60 seconds and are invalidated by resolution, task/session end, or lock.

The panel binds only to loopback, uses a random session credential, and has
bounded requests. Its credential is separate from the MCP session credential and
is not included in agent instructions. `acc approvals --url` deliberately prints
the private link for opening manually; treat it as a credential. This is a local
desktop control, not a phone/LAN service. Same-account software with filesystem
or browser access is not cryptographically excluded; native sandbox and OS
permissions remain the boundary.

Typed `/approve N`, `/deny N`, `/worker-approve N`, `/worker-deny N` remain
available. `/answer N JSON` and `/worker-answer N JSON` submit an answer object,
for example `{"direction":{"answers":["Research"]}}`. All structured answers
require a deliberate human control: the Codex request format can also carry
connector consent and does not reliably distinguish it from an ordinary question.
Spoken “yes”, “approve” or “option two” cannot grant permission or answer that form.
Secret-input, authentication/device-proof, and unsupported permission-grant
requests continue to fail closed.

Project permission profiles provide a deliberate launch policy:

```sh
acc jobs profile set research --workspace /path/to/project
acc jobs profile set coding --workspace /path/to/project --write --reviewer auto
acc jobs profile list
acc run --workspace /path/to/project --project-profile coding
acc jobs profile remove coding
```

Profiles default to read-only and human review. `--write` opts into workspace
writes; `--reviewer auto` selects the backend's existing automatic reviewer.
Activation is explicit and must match the exact canonical workspace.
Profiles and credentials are not exposed through settings-update MCP or speech.
Removing a profile affects future launches; stop an existing process to revoke
its current authority. Harness behavior differs: `/capabilities` states what each
adapter actually implements, including Antigravity's noninteractive permission
mode. A profile does not bypass backend review or change connector authorization.

Codex steering and resume follow its [official app-server protocol](https://learn.chatgpt.com/docs/app-server).
Steering targets the current turn ID, reports acceptance only after a successful
backend response, and leaves work running when unsupported or rejected. Preview
text never executes Accessor directives; completed replies remain authoritative.
Worker commentary is spoken at most once per 20 seconds and shortened for speech;
tool detail and full results stay on screen. `/quiet` suppresses output.

## Word-for-word voice test

Run these from your project directory, using your existing Codex login:

```sh
acc --version
acc doctor
acc tts test
acc run --agent codex --wake-code 29 --speak --tts piper --idle-seconds 0 --workspace .
```

The version should be `0.37.0`. Start with a headset in a quiet room. Unlock first
if your profile has a password. Say only the quoted lines below; bracketed text
is an instruction. “Hey twenty-nine” works with attention mode enabled or
disabled. Wait for the chime before the request, and leave at least three seconds
between draft segments so each becomes a separate clip. If recognition is slow,
wait until the segment appears in Activity before continuing.

### 1. Everyday conversation

> “Hey twenty-nine.”
>
> [Wait for the chime.]
>
> “What is twelve plus thirty?”

Expect an answer of **42**. After the reply finishes:

> “Give me one practical tip for keeping my desk tidy.”

Expect an ordinary follow-up without another wake phrase.

### 2. Interrupt and resume speech

> “Hey twenty-nine.”
>
> [Wait for the chime.]
>
> “Explain how rain forms in six sentences.”
>
> [When the spoken answer starts, interrupt it.]
>
> “Hey twenty-nine.”
>
> [Wait for the listening cue.]
>
> “Stop talking.”
>
> [Wait three seconds.]
>
> “Continue speaking.”

Expect output to stop and then resume. The interrupted synthesis chunk may
repeat; completed chunks should not. If the whole answer finished before you
interrupted, repeat this step rather than expecting a paused reply to exist.

### 3. Draft with pauses and a correction

> “Hey twenty-nine.”
>
> [Wait for the chime.]
>
> “Let me explain.”
>
> [Pause at least three seconds after each following line.]
>
> “Make a checklist using only the activities I list.”
>
> “Drink coffee.”
>
> “Scratch that.”
>
> “Wait.”
>
> [Stay silent for five seconds.]
>
> “Take a short walk.”
>
> “Stretch for one minute.”
>
> “Send that.”

Expect no agent submission before “send that”, and one combined request
containing the walk and stretch without the removed coffee segment.

### 4. Background research while you keep talking

> “Hey twenty-nine.”
>
> [Wait for the chime.]
>
> “Let me explain.”
>
> [Pause at least three seconds after each following line.]
>
> “Start a background research task comparing three local-first voice assistant architectures.”
>
> “Compare wake detection, speech recognition, interruption handling, and privacy. Use at least six primary sources.”
>
> “Delegate it to a separate Codex worker so we can keep talking. Do not modify files.”
>
> “Send that.”

Wait until Activity shows a worker/task starting; note its ID. Then, while the
worker is still running, say:

> “Hey twenty-nine.”
>
> [Wait for the listening cue.]
>
> “Task status.”
>
> [Wait for the status reply to finish.]
>
> “What is seven times eight?”
>
> [Wait for the answer.]
>
> “Hey twenty-nine.”
>
> [Wait for the listening cue.]
>
> “Tell the task focus on Linux and distinguish measured results from estimates.”

Expect a local running/waiting status, an answer of **56**, and an accepted-steer
message from Codex while the research continues. Waking or silencing must not
cancel it. Speech-driven delegation depends on the main agent; if it works inline
instead, cancel that task and use this typed fallback to exercise the worker
controls deterministically:

```text
/task new Research three local-first voice assistant architectures. Compare wake detection, speech recognition, interruption handling and privacy using at least six primary sources. Do not modify files.
```

The fallback uses the configured Coding harness; select Codex there if testing
live steering and native resume. One worker runs at a time.

### 5. Catch up, silence, then explicitly cancel

While that worker is still running:

> “Hey twenty-nine.”
>
> [Wait for the listening cue.]
>
> “Catch me up.”
>
> [When the update is speaking, interrupt again.]
>
> “Hey twenty-nine.”
>
> [Wait for the listening cue.]
>
> “Stop talking.”
>
> [Wait three seconds.]
>
> “Task status.”
>
> [Wait for the status reply to finish.]
>
> “Cancel the task.”
>
> [Wait three seconds.]
>
> “Task status.”

Expect the worker to survive waking and silencing, then stop only after the
explicit cancellation in this step. The final status should report no active
task. If research already finished, start another task before testing cancellation.

### 6. Saved work and explicit resume (typed controls)

Type `/jobs`, note the research task ID, then `/quit`. Relaunch the same command
from the same project and type:

```text
/jobs
/task show TASK_ID
/task resume TASK_ID Inspect the existing state and continue the research. Do not modify files.
```

Replace `TASK_ID` with that ID. Expect the receipt to survive, no automatic
restart, and a new active worker only after the resume command. Then repeat the
spoken “task status”, steering and cancellation checks. A saved read-only task
must remain read-only.

### 7. Coding and human controls

For a coding inspection in a disposable Git project, launch there and type:

```text
/task new --isolated Inspect this project and propose three small code improvements. Do not modify files.
```

While it runs, say “hey twenty-nine”, wait for the cue, then “tell the task
prioritize test coverage”. Inspect `/task show ID` and `/task patch ID`; the saved
execution workspace should be a detached worktree. To test actual edits, select
a writable launch policy deliberately and give a specific edit in that disposable
project. Uncommitted source changes are not copied into an isolated worktree.

When a backend question or permission request occurs, open `/approvals`. Say
“yes”; the request should remain pending. Use the panel's labelled answer or
permission control before its 60-second expiry, and verify that it resolves.
This requires an actual backend request; an ordinary text question does not
necessarily produce a structured form.

Record the phrase, whether the task was running or speaking, room conditions,
expected behavior and actual behavior for failures. `/logs` shows the diagnostic
log location; enable logging in Settings → Display if needed. Repeat wake and
interruption checks using speakers, music and fan noise. Also listen to ordinary
conversation that does not address Accessor, noting missed wakes, accidental wakes,
clipped words and response delay. The [voice experiments](voice-experiments.md)
provide optional measured corpus tests.
