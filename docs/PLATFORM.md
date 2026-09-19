# Accessor platform

Accessor is a local voice shell around **harnesses** (Codex, Claude Code, Antigravity, mock). It owns the microphone, wake/sleep, speech, the dashboard, and routing. Each harness owns tools, sandbox, and **its own conversation thread**.

## Audio overflow

`Audio buffer overflow; restart Accessor before issuing more commands` means the microphone callback filled a bounded queue faster than the DSP thread could drain it (CPU spike, a long burst, or a stall). That used to **kill capture**. Accessor now drops the overrun, resets the segmenter, and keeps listening. One occurrence is noisy, not fatal. Persistent repeats mean the machine cannot keep up — shorter turns, a quieter room, or fewer other CPU-heavy jobs.

Utterances longer than 15 seconds are still discarded on purpose (not executed truncated).

## Wake vs conversation STT

| Stage | Default | Optional |
| --- | --- | --- |
| Wake word (`29`, `hey 29`) | **Internal** local STT (`stt.engine`, default Canary) | Always local — never cloud. Speech → Local STT model opens a list (Canary, Parakeet TDT 0.6B INT8, Whisper Tiny/Base/Small Q5). Esc leaves the list with no download. Only a missing model you then pick asks to confirm size. |
| After the session is awake (GREEN) | Same local engine | **External** Cartesia **Ink-2** (`stt.conversation=cartesia`) |

`/tts provider cartesia` is **spoken output** (Sonic). `/stt provider cartesia` (aliases: `ink-2`, `external`) is **after-wake transcription**. `/stt engine parakeet` (or Speech → Local STT model) selects the on-device model. Wake spotting cannot use Ink-2.

Ink-2 is **not a live stream**. The 120s idle timer is only sleep. After GREEN:

1. Earshot VAD closes an utterance (with ~320ms preroll).
2. The selected local STT model must hear a real word on that clip.
3. Only then is that same buffered WAV posted to Cartesia Ink-2.

Noise, silence, and unfinished speech never hit the cloud. Utterances over 15s are dropped.

**Compute saving** (`stt.lazy`, default off): the microphone and Earshot VAD always run, and the segmenter already keeps **320ms of audio before the spike**. The local STT session is the expensive part (Canary ~180MB; Parakeet INT8 ~640MB encoder). When this is on, weights load on the first ~160ms of voiced audio (usually while you are still talking) and drop ~45s after you go back to WHITE. First wake after unload can add a fraction of a second if the phrase is shorter than the load. ONNX Runtime itself stays in process; only the selected STT sessions are freed. The Ink-2 word-gate still needs that local model for that one clip.

## Harnesses

Settings → **Harnesses**:

- **Plugin** — CLI that holds Gmail, Calendar, Docs, and other connectors. Used when the turn needs those apps.
- **Coding** — repos, diffs, tests, refactors (typically Codex).
- **Everyday** — general chat: time, planning, questions that are not code and not a plugin. Its model picker lists **that CLI's** models (Gemini if everyday is Antigravity).
- **Router** — `off` (always everyday), `keywords`, or `jev`.

`jev` is **TypeSafe Jev**: a System One *evaluator*, not a chat model. It returns typed choices/probabilities in ~100ms. Accessor asks plugin vs coding vs everyday. That selects the harness for that slot. It does not invent a free-form model id.

If coding and everyday are the same CLI, Jev still runs: plugin turns go to the plugin harness, everything else stays on that shared CLI. Previously, matching coding==everyday skipped routing entirely and stuck on the plugin CLI.

- TypeSafe key (`TYPESAFE_API_KEY`, `/jev key` in the dashboard, `acc jev key`, or OS secret `typesafe`): required for Jev itself (`POST https://api.typesafe.ai/v1/systemone`). The key is never written to settings.json.
- Vercel AI Gateway key (`AI_GATEWAY_API_KEY` or secret `ai-gateway`): **not** Jev. Used to compact Accessor-owned history with `routing.compaction-model` (Gemini Flash / GPT 5.6 Luna / Claude Haiku, etc.) when a new harness needs a handoff summary.

**Jev auto-select** (`routing.auto-model`): when a TypeSafe key is present, Accessor uses Jev even if the router dropdown is still on keywords. Without a key, keywords are used.

If Jev is not configured, **keywords** send email/calendar/docs to the plugin harness, code-like turns to coding, and the rest to everyday.

### Conversation history

- **Same harness, model switch** (e.g. Codex Sol → Astra): the Codex *thread* stays. History continues. Codex auto-compacts when the context window fills. Accessor can also summarize its own rolling transcript with the compaction model (Gateway) when handing context to a **new** harness.
- **Harness switch** (Codex → Antigravity): these are **different processes**. There is no shared token window. Accessor keeps **both** sessions warm in one run so each side remembers its own turns. The first prompt on a newly started harness may include a short handoff of recent User/Agent lines (compacted if long). The dashboard always shows **which harness and model** is live.
- Accessor does not merge the two native token windows into one. Each CLI still only sees what it was sent.

Spoken identity (default on): spoken **letters** (`A. D.`) at most **once every 5 minutes**. Later replies in that window skip the prefix. TTS sees `X. S.` so it does not mash them into “Adee”. `X S` on screen = Codex Sol, `C F` = Claude Fable, `A F` = Antigravity Flash, `M D` = mock default. Toggle Display → “Say harness code first”.

TTS sanitizes markdown/symbols (pipes become a comma, not “vertical bar”) and **starts speaking in sentence chunks**, synthesizing the next chunk while the current one plays.

The **plugin harness** (`agent`) is the connector CLI. **Everyday** is general chat (router `off` uses it). **Coding** is engineering.

`acc update` (or `/update` in the dashboard) runs each **found** harness's own updater: `codex update`, `claude update`, `agy update`. `/update check` and `acc update --check` only print `--version`. Accessor closes warm harness sessions first so Windows can replace the binary. Restart Accessor afterward. Mock is skipped.

### Copying this machine to another

`acc config locations` prints the folder to copy. On Windows that is typically `%APPDATA%\Accessor` (Roaming). It holds `config.json` plus optional `analytics.json`, `models.json`, `tts-cache/`, and `events/`. Secrets are **not** in that folder — they live in the OS credential store (Windows Credential Manager, service `Accessor`). Re-enter `acc tts key`, `acc jev key`, and any AI Gateway key on the new machine. Speech models live under the assets directory (`ACC_ASSETS` or `assets-dir` in config); copy that too or re-run `python scripts/setup_speech.py`. Codex / Claude / agy installs and their plugin logins stay with those CLIs.

Handoff: say “switch to Codex” (or “switch agent to claude”) to pin **this conversation** to that CLI. A running harness can also emit `ACCESSOR_SWITCH harness=codex` (optional `model=…`) or JSON `{"accessor_switch":{"harness":"codex"}}` after seeing the available CLI list in its instructions. That pins the live session; it does **not** rewrite the plugin/coding/everyday slots. Free-form agent claims do not change settings. Say “switch to plugin” to jump to the connector CLI.

Saved **voice metaprompt** (`prompt`) is sent to Codex, Claude, and Antigravity. `/config set prompt default` restores the hands-free default. It persists in `config.json`.

## Analytics, context, compaction

`/analytics` shows **lifetime** totals and a **past 7 days** window: Jev calls, TTS, STT, harness turns, compaction, and estimated USD (Cartesia / Jev / Gateway list prices; harness tokens are unpriced). Events persist in `analytics.json` under Accessor’s home.

`/context` shows approximate tokens on Accessor-owned history. `/compact` summarizes it now. Settings → Harnesses: pick a **compaction provider**, then a **model from that list**, and a **token threshold** (default ~4000). Auto-compact runs when the rolling transcript exceeds that. If an AI Gateway key is present, Gateway does the summary; otherwise Accessor keeps a local extractive trim.

Antigravity (`agy`) is driven with `--input-format stream-json`: each turn is `{"event":"user","message":{"content":"..."}}`. Replies come from `event: result` → `result.response`. If no model is set, Accessor passes **`gemini-3.8-flash`**. Accessor pipes harness stderr into Activity (auth/login/errors) and treats a process exit while BLUE as a visible failure instead of hanging. Look for `agy.exe` under `%LOCALAPPDATA%\agy\bin` if it is not on PATH.

Settings → Voice: **think warble**, **wake chime**, and **sleep chime** volumes (0 silent, 1 default). The warble plays while BLUE and stops when speech starts or you barge in. Status chrome — every box outline on the page — is **green** while the conversation is open, **blue** while the agent works, and **purple** while TTS is speaking. Activity scrolls with the **mouse wheel** as well as PgUp/PgDn.

### Caching

Harness processes stay warm in one run — that is the conversation cache. Prompt-cache headers do not transfer if you switch Codex ↔ Claude ↔ Antigravity, and Accessor does not speak those vendors’ APIs directly, so we do not fake a shared KV cache across providers. Short Cartesia/Kokoro clips (identity letters, repeated sentences) are stored under `tts-cache/` so the next identical chunk does not hit the network.

Reasoning (`routing.reasoning`: default/low/medium/high) is passed to Codex as turn `effort`. Antigravity always passes `--effort` with `--model` (`default` maps to `low`; gemini-3.8-flash requires it). Claude Code and Antigravity receive `--model` when set. Say “use high reasoning” to change it; the next harness process picks it up.

## Approvals

Codex default is **`approvalsReviewer: auto_review`** with `approvalPolicy: on-request`. The Guardian/auto-review classifier handles sandbox escalations. Typed `/approve N` remains the human override. Claude and Antigravity use each CLI’s auto/accept policy when that harness is selected (`--permission-mode` / `--dangerously-skip-permissions` is **not** the default; we prefer the harness’s own auto-reviewer when it exists).

## Settings

Interactive dashboard: **↑/↓** move, **Enter** opens a category or toggles, **Esc** goes up/closes. The selected row shows a cyan description of what that setting does. Harness pickers list which CLIs are actually on PATH. Categories: Voice, Speech, Harnesses, Display, Tests. Typed `/settings KEY VALUE` still works for scripts.

## What you need installed

1. Rebuild: `.\install-accessor.ps1` or `cargo build --release --bin acc`
2. `acc doctor` — local STT files + ONNX
3. `acc tts setup` — system / Kokoro / Cartesia
4. At least one harness login: Codex (`acc agent`), optional `claude`, optional `agy`
5. Optional: Cartesia key (TTS and Ink-2), TypeSafe or AI Gateway key (Jev / compaction)
6. `acc -wakecode 29 speak`
