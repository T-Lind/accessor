# Harnesses

A **harness** is a command-line coding agent that Accessor drives. Accessor owns
the microphone, wake/sleep, speech, the dashboard, and routing; the harness owns
its tools, sandbox, permissions, and its own native conversation thread.

## Supported harnesses

| Id | CLI | Notes |
| --- | --- | --- |
| `codex` | Codex App Server | Default on Windows; uses your existing login. Accessor never calls the OpenAI API directly |
| `claude` | Claude Code | Uses the installed CLI and its login |
| `antigravity` | `agy` | Passes the active workspace to its server |
| `opencode` | `opencode` | Read headless; management commands are interactive |
| `cursor` | `cursor-agent` | Read headless; reads `.cursor/mcp.json` |
| `mock` | none | No account needed; useful for testing the plumbing |

A "gateway" and "local" compaction path also exist for summarization without a
harness. Model discovery reads the selected Codex installation's account catalog
without running an LLM; the last successful list is cached for offline display.

## Roles

Three roles can use the same or different harnesses and models:

- **Main** — the persistent conversation, with a lightweight model by default.
- **Coding** — isolated workers for code and difficult analysis.
- **Plugin** — connector/plugin work; defaults to following Main.

```sh
acc config set routing.main claude
acc config set routing.main-model haiku
acc config set routing.coding codex
acc config set model gpt-5.6-sol
```

The metaprompt tells the main agent to delegate coding, difficult analysis, and
plugin work — even when all roles use the same CLI. A worker gets a fresh process,
its own model and reasoning level, a 15-minute deadline, and returns a **bounded**
result to the main conversation. Only one worker runs at a time, and worker
controls cannot recursively delegate. See
[Routing and delegation](architecture.md#routing-and-delegation).

## Models and reasoning

Each harness→model→reasoning combination is selectable in
`/settings` → Harnesses. Say "switch to Codex" or "switch model to Astra" using a
name from your discovered list. A running harness can also emit
`ACCESSOR_SWITCH harness=codex` (or JSON `accessor_switch`) to pin this
conversation to another CLI without rewriting the plugin/coding/main slots.
Accessor passes the switch through the same wake-code and barge-in rules.

**Fast mode** (`routing.fast-mode`) is a best-effort low-latency path: it forces
low effort and each harness's light model for the main conversation and workers.

## Connectors are reused

Accessor does not implement a second Gmail client, OAuth store, or connector SDK.
Configure connectors in the agent itself:

```sh
acc connectors setup
acc connectors list
acc connectors status
acc connectors refresh
```

- **Codex** opens its native UI for setup and asks App Server for accessible apps.
- **Antigravity** uses its native plugin catalog and MCP registry.
- **opencode** / **Cursor** have interactive management, so Accessor reads their
  own configuration (`opencode.json` `mcp`/`plugin`, `.cursor/mcp.json`
  `mcpServers`) and injects the cached inventory into each launch.

Restart Accessor after changing integrations. Status reports
configuration/runtime availability, not a completed external tool call.

## Accessor's MCP server

`acc mcp` serves Accessor's own tools over stdio. Codex and Claude launched by
Accessor receive a session-specific connection; existing connectors remain
available. `acc mcp-install` repairs registration manually.

| Tool | Purpose |
| --- | --- |
| `memory` | Search and maintain shared facts |
| `notes_search` / `note_read` / `note_delete` | Private Markdown notes (delete needs an exact ID and user authorization) |
| `organizer_control` | Notes, alarms, tasks, sleep, `stop_alarm`, `list_schedules` |
| `notifications` | List, read, dismiss, read-all |
| `harness_health` | Executable discovery, configured roles, workspace, cached model count, timezone |
| `usage_status` | Subscription quota observations (cached by default) |
| `settings_read` / `settings_update` | Narrow, validated preference surface |
| `session_control` | Sleep and ringing-alarm stop through an authenticated loopback bridge |
| `delegate_task` | Start an isolated worker; the result returns asynchronously |

Registration preserves unrelated entries and refuses name collisions. Native
harness tool permissions still apply. Session capabilities are supplied to main
sessions, not to isolated workers or compaction jobs.

## Permissions and approvals

The default Codex sandbox is read-only; `--workspace-write --workspace PATH`
permits workspace edits under Codex's policy. Structured command/file approvals
and empty-form MCP confirmations require a typed `/approve N` and expire after 60
seconds. Authentication and device-verification requests, forms requiring field
values, and permission-profile grants are declined rather than guessed — complete
those in the harness's native interface.

See [Features → Permissions and scope](features.md#permissions-and-scope).
