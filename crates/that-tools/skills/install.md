---
name: anvil-install
description: Install and configure the anvil CLI. Use this skill when setting up anvil for the first time, adding it to a project, or configuring policy and search engines.
---

# Installing and Configuring Anvil

Anvil is a single Rust binary that gives any agent structural code comprehension, federated web search, persistent memory, and human-in-the-loop governance — all through a plain CLI.

## Install

```bash
# From crates.io
cargo install anvil

# From source
git clone https://github.com/agentcommerce/agentic-tools
cd agentic-tools && cargo build --release
# Binary is at target/release/anvil
```

Verify:
```bash
anvil --version
```

## Initialize a project

Run once in any project root to create a `.anvil/config.toml` with sensible defaults:

```bash
# Recommended for agent use (allows search, memory, code reads; prompts for writes)
anvil init --profile agent

# Strict defaults (most actions require approval)
anvil init --profile safe

# All tools allowed without prompting (use in trusted sandboxes only)
anvil init --profile unrestricted
```

## Configure search engines

Set free API-key-less engines (default), or add keys for premium engines:

```bash
# .anvil/config.toml
[search]
primary_engine = "duckduckgo"          # free, no key needed
fallback_chain = ["bing", "yahoo", "mojeek", "tavily", "brave"]

# For Tavily (best quality, neural search):
# Set env var: TAVILY_API_KEY=tvly-...

# For Brave:
# Set env var: BRAVE_API_KEY=BSA...
```

Or pass `--engine` at call time:
```bash
anvil search query "rust error handling" --engine bing
```

## Install skills for agent auto-discovery

One command installs all skills as `SKILL.md` files so any agent framework auto-discovers them:

```bash
# Install all skills to ~/.claude/skills/ (default — Claude Code convention)
anvil skills install

# Install to a custom agent skills directory
anvil skills install --path /path/to/agent/skills/

# Install a single skill
anvil skills install code
anvil skills install search --path ~/.claude/skills/

# Re-install and overwrite existing files
anvil skills install --force
```

Each skill is written to `<dest>/anvil-<name>/SKILL.md`. Example:
```
~/.claude/skills/
  anvil-code/SKILL.md
  anvil-fs/SKILL.md
  anvil-search/SKILL.md
  ...
```

Output: JSON array with `name`, `path`, and `skipped` (true if file existed and `--force` was not set).

Once installed, Claude Code and compatible agent frameworks auto-load skill descriptions into context. The agent will know how to use the `anvil` CLI without any MCP server or additional configuration.

## Read skills on demand

Any time you want the agent to know how to use a tool category, output the skill:

```bash
anvil skills read search    # search and fetch documentation
anvil skills read code      # code reading, grep, edit, AST tools
anvil skills read memory    # persistent memory operations
anvil skills read fs        # filesystem operations
anvil skills read exec      # shell execution
anvil skills read human     # human-in-the-loop approval flows
anvil skills list           # see all available skills
```

## Policy enforcement

Anvil enforces per-tool policies from `.anvil/config.toml`. In headless mode (no TTY), any tool with policy `prompt` is automatically denied. Configure accordingly:

```toml
[policy.tools]
code_read  = "allow"    # agent can always read code
fs_write   = "prompt"   # ask human before writing files
fs_delete  = "deny"     # never delete files
shell_exec = "allow"    # allow exec (only in trusted environments)
```

## Global configuration

Global config lives at `~/.config/anvil/config.toml` and applies to all projects. Project config at `.anvil/config.toml` overrides it. Environment variables (`ANVIL_SEARCH__PRIMARY_ENGINE`, etc.) override both.
