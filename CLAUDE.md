# that-agent Workspace

Rust workspace containing `that-tools`, `that-core`, `that-cli`, and `that-eval`.
The `that-agent/` and `agentic-tools/` directories are standalone projects — they are **not** workspace members.

## Workspace Layout

```
crates/
  that-tools/    — tool implementations (fs, code, exec, search, memory, skills, human)
                   + config loading, daemon, index, output formatting
  that-core/     — orchestration, preamble builder, skills discovery, workspace model,
                   session manager, heartbeat, channels integration, sandbox routing
  that-channels/ — Channel trait, ChannelRouter (fan-out), adapters (Telegram, Discord, …)
  that-cli/      — CLI binary wrapping that-core
  that-eval/     — headless agent eval harness (scenarios, judge, reports)
evals/
  scenarios/     — TOML eval scenario files
sandbox/
  Dockerfile     — multi-stage sandbox image (Rust builder + python:3.12-slim)
  build.sh       — builds the Docker image from workspace root
that-agent/      — standalone TUI agent (separate Cargo workspace)
agentic-tools/   — standalone tooling (separate Cargo workspace)
```

## Gotchas & Lessons Learned

### rsync Path Anchoring in build.sh

Always use a leading `/` on rsync excludes to anchor them to the source root:

```bash
rsync -a \
    --exclude='/target' \
    --exclude='/.git' \
    --exclude='/sandbox' \
    "$PROJECT_DIR/" "$BUILD_CTX/"
```

Without the `/`, `--exclude='sandbox'` will also match `crates/that-core/src/sandbox/` (the Rust module directory), silently stripping it from the build context. The Docker build then fails with `error[E0583]: file not found for module sandbox`.

### Workspace File Model (Soul/Identity Split)

`inner/mod.rs` is kept for backward compatibility (agents with existing `Inner.md` files).
All new code uses `workspace/mod.rs`. The split:

| File | Role | Edit frequency |
|---|---|---|
| `Soul.md` | Deep identity: character, values, philosophy | Slow — evolves with the agent |
| `Identity.md` | Shallow: name, vibe, emoji | Bootstrap-created, rarely changes |
| `Agents.md` | Operating instructions, tool discipline, memory, heartbeat | Agent-editable at any time |
| `User.md` | Who the user is and how to address them | Grows organically |
| `Tools.md` | Local env notes: devices, SSH, preferences | Environment-specific |
| `Boot.md` | Optional startup checklist | Optional |
| `Bootstrap.md` | First-run ritual — **ephemeral**, agent deletes it on completion | One-time |

**`generate_soul_md()`** generates a combined output with Identity sections first (Name, What I Am,
Vibe, Emoji) then Soul sections (Character onward). `split_identity_soul()` splits at `## Character`.

**Preamble template vars**: `Agents.md` supports `{max_turns}` and `{warn_at}` placeholders
substituted at preamble build time.

### Workspace File Model — Agent Self-Knowledge Files

The full runtime layout the agent can navigate via `fs_ls`:

```
~/.that-agent/agents/<name>/
  Soul.md          — deep identity: character, values, philosophy (slow evolution)
  Identity.md      — shallow: name, vibe, emoji (bootstrap-created, rarely changes)
  Agents.md        — operating instructions, tool discipline, heartbeat, tasks (agent-editable)
  User.md          — who the user is; grows organically
  Tools.md         — local env cheat sheet: devices, SSH, preferences (instance-specific)
  Memory.md        — thin navigation index pointing into memory.db (updated after compaction)
  Boot.md          — optional startup checklist
  Bootstrap.md     — first-run ritual; agent deletes it on completion
  Heartbeat.md     — scheduled autonomous work entries
  Tasks.md         — task index for epic/story/task hierarchy
  tasks/           — folder-based task hierarchy (epic-NNN/, story-NNN/, task-NNN.md)
  skills/          — agent-scoped SKILL.md files (hot-reloaded, no restart needed)
  plugins/         — plugin directories with plugin.toml manifests
  config.toml      — LLM & channel settings (auto-reloads on change)
  memory.db        — SQLite FTS5 persistent memory store (per-agent, not shared)
  .bashrc          — shell profile for environment exports
~/.that-agent/workspaces/<name>/   — isolated project workspace (bind-mounted at /workspace in sandbox)
```

### Skill Frontmatter Requirements

`parse_frontmatter()` in `that-core/src/skills/mod.rs` requires `name:` and `description:` at the **root** of the YAML frontmatter block. Fields nested under a `metadata:` key are not recognized. A skill with missing root-level fields is silently skipped during discovery — the agent will never see it.

Minimal valid SKILL.md frontmatter:
```yaml
---
name: my-skill
description: What this skill does
triggers:
  - keyword phrase
---
```

### Skill Hot-Reload and Silent Eligibility Filtering

New or updated SKILL.md files under `~/.that-agent/agents/<name>/skills/` are picked up on the next agent run — no restart required.

Skills that fail eligibility checks are **silently skipped** — they never appear in the agent's catalog. Checks run in this order:
1. OS (`darwin`, `linux`, `win32`) — empty list means any OS
2. `binaries` — **all** listed binaries must be on PATH
3. `any_bins` — **at least one** must be on PATH
4. `envvars` — **all** env vars must be set (supports `${VAR}` and `ALIAS: ${VAR}` syntax)

If a skill mysteriously vanishes from the catalog, check eligibility — a missing binary or unset envvar is the most common cause.

### Skill Progressive Disclosure Model

Skills have three layers, each loaded on demand:

| Layer | When loaded | What it contains |
|---|---|---|
| Frontmatter metadata (`name`, `description`) | Always — injected into preamble catalog | Enough for the agent to recognize when a skill applies |
| SKILL.md body | On demand via `read_skill <name>` tool | Step-by-step instructions, design patterns |
| `references/` directory files | On demand via `read_skill <name> <ref>` | Deep reference material, exhaustive examples |

Keep SKILL.md body under 400 lines. Put exhaustive detail in `references/`. This controls context budget.

### `always: true` Skills — Preamble Injection vs. Catalog

A skill with `always: true` in its frontmatter metadata is **injected inline as a named section** into every preamble — it never appears in the "Available Skills" catalog and the agent never needs to call `read_skill` for it.

`channel-notify` is the canonical example: it is always injected because the agent must always know how to notify the human operator.

Use `always: true` sparingly — it consumes preamble tokens unconditionally. Prefer `always: false` (the default) and let the agent load skills progressively.

### Policy Enforcement — ALL Execution Paths

`load_agent_config(container)` must be called in **every** agent execution path. The codebase currently has three:

| Function | File |
|---|---|
| `execute_agent_run_streaming()` | `that-core/src/orchestration.rs` |
| `execute_agent_run_tui()` | `that-core/src/orchestration.rs` |
| `execute_agent_run_eval()` | `that-core/src/orchestration.rs` |

Missing it from any one path means that path silently uses the default restrictive policy even in sandbox mode. The symptom is `policy denied: tool 'fs_delete' is not allowed` despite the scenario setting `sandbox = true`.

### Sandbox vs. Host Policies

Destructive tools (`fs_delete`, `shell_exec`, `fs_write`, `code_edit`, `git_commit`, `git_push`) default to **Deny** on the host. They should only be elevated when `container.is_some()` (sandbox mode). The Dockerfile already sets `THAT_TOOLS_POLICY__TOOLS__*=allow` env vars inside the container, but those only affect the in-container process — the host-side policy check in `load_agent_config()` is what gates sandbox-mode runs from the eval harness.

### Memory.md Update Contract

After every `mem_compact` call the agent **must** update `Memory.md`:
1. Append a row to the `## Compaction Summaries` table (date, topic, recall query)
2. Refresh the `## Active Topics` line
3. Optionally add or update a `## Quick Recall Hints` entry

`Memory.md` is a thin pointer index — never paste full content into it. Full content lives in `memory.db` and is fetched via `mem_recall`. If `Memory.md` drifts out of sync, the agent's ability to find its own past knowledge degrades silently.

Each agent has an isolated `memory.db` at `~/.that-agent/agents/<name>/memory.db`. There is no cross-agent memory sharing.

### Session History Anchoring

`rebuild_history_recent(entries, max_pairs)` anchors history reconstruction at the **last `Compaction` event** in the transcript, then replays only the most recent message pairs from that point. This prevents context bloat on session restart.

If a session has never been compacted, all pairs are replayed up to `max_pairs`. When a session grows large and the agent notices context pressure, it should call `mem_compact` to create an anchor point before the next restart.

### Heartbeat Dispatch Rules

- `urgent` priority entries fire **immediately on first dispatch** (no `last_run` yet), then follow the configured schedule thereafter.
- All other priorities only fire when `last_run + interval <= now`.
- Use `status: running` for active recurring work. Set `status: done` only to permanently disable an entry.
- Prefer Heartbeat schedules over installing system-level cron daemons — the harness manages timing; the agent just edits `Heartbeat.md`.

Supported schedules: `once | minutely | hourly | daily | weekly | cron: <expr>`

### Channel Router Architecture

`ChannelRouter` broadcasts outbound events (stream tokens, tool calls, notifications, done) to **all enabled adapters simultaneously** (fan-out).

`human_ask` is the exception: it routes to the **first adapter that declares `ask_human` capability**. All other adapters receive a "waiting for human input" notification during the blocking wait.

`TuiChannel` lives in `that-core/src/tui.rs` (not in `that-channels`) to avoid a circular dependency between `that-channels` and `that-core`.

Channel sessions are persisted in `channel_sessions.json`, mapping sender-key (e.g., Telegram chat ID) → session ID. This restores conversation context when the bot restarts.

### Eval Scenario Design Principles

**Test autonomy, not tool knowledge.** Prompts in eval scenarios must read like a human making a request — they must never name specific tools, skill names, or internal workflows. The agent must decide on its own how to accomplish the task.

Bad: `"Use the skill-creator skill to create a skill called 'json-formatter'."`
Good: `"I'd like you to create a new skill for yourself that helps with writing well-formatted JSON files."`

Bad: `"Use shell_exec to run the git commands."`
Good: `"Set up a Python CLI project at that path with git version control."`

**Test native workflows end-to-end.** Using a `create_skill` step to plant a skill and then asking the agent to use it tests skill invocation only — not skill creation. If the scenario evaluates skill authoring, the agent must be the one to create the skill. Bypassing native workflows gives the judge false signal and produces inflated scores.

**Judge feedback must be actionable.** The rubric `description` fields and any judge rationale should suggest concrete, NLP-level improvements to the agent's prompt or reasoning — not vague notes like "agent did not use the right tool." Good feedback explains *why* the agent missed the goal and *how* the preamble or skill instructions could be reworded to guide it better.

### Unicode-Safe String Truncation

Never use `&str[..n]` for truncation — it panics if the byte offset falls inside a multi-byte codepoint (em-dash `—` = 3 bytes, emoji = 3-4 bytes).

Always use char-based slicing:
```rust
// Truncate to 120 visible chars, strip control chars
let clean: String = s.chars().filter(|c| !c.is_control()).take(120).collect();
```

For finding a byte offset safely:
```rust
let truncated = match s.char_indices().nth(800) {
    Some((i, _)) => &s[..i],
    None => &s,
};
```

### Docker Build Context Must Include the Full Workspace

`cargo build` requires the full workspace `Cargo.toml` and all member crates to be present in the build context. Do not try to copy only a subset of crates — rsync the entire workspace root into a temp dir and use that as the Docker build context.

### Sandbox Filesystem Routing (FIXED)

In sandbox mode **all filesystem tools** (`fs_ls`, `fs_cat`, `fs_write`, `fs_mkdir`, `fs_rm`) AND `shell_exec` now route through `docker exec` into the container. The agent's reads and writes are fully consistent — files written with `fs_write` are immediately visible to `shell_exec` git commands and vice versa.

Relative paths are anchored to `/workspace` (the container's declared working directory). `/workspace` is bind-mounted from `~/.that-agent/workspaces/{agent_name}` on the host, so the host can also read those files if needed.

**Eval assertions**: use `docker exec $THAT_EVAL_CONTAINER_NAME ...` inside scenario `run_command` / `assert.command_succeeds` blocks. The eval runner injects `THAT_EVAL_CONTAINER_NAME`, `THAT_EVAL_AGENT_NAME`, `THAT_EVAL_WORKSPACE`, and `THAT_EVAL_SANDBOX` for portable assertions across agents.

### Scenario `sandbox = true` for Destructive Steps

Any eval scenario that requires file deletion, directory removal, or unrestricted shell execution must set `sandbox = true` at the top of the TOML. The runner uses this flag to elevate policies. Failing to set it means the agent will be blocked mid-scenario.

### Channel Streaming Buffer Hygiene

Channel adapters that buffer streamed text tokens must clear the buffer on every tool-call boundary. The agent loop emits text deltas for **all** turns — including intermediate reasoning turns that narrate tool usage. If the buffer accumulates across turns, the final "done" event sends the entire multi-turn narration instead of just the clean user-facing response. Clearing on tool-call ensures only post-last-tool text survives to delivery.

### `.env` Files and Secret Hygiene

- The root `.gitignore` excludes `.env` and `.env.*` (except `.env.example`).
- `that-eval/main.rs` calls `dotenvy::dotenv()` early, before any config loading. It silently succeeds if `.env` is absent.
- Never commit `.env` files. If API keys are accidentally exposed in git history, rotate them immediately.

## Model Preferences

For OpenAI provider runs (CLI flags or scenario overrides), always use **`gpt-5.1-codex-mini` or higher** — never `gpt-4.x` series models. Preferred default: `gpt-5.2-codex`.

## Agent / Skill Prompt Guidelines

When writing agent preambles, skill instructions, or eval prompts, use NLP-driven, generic language. Never include real file paths, real component names, or real model IDs as examples — keep instructions intentional and abstract so they transfer across environments.
