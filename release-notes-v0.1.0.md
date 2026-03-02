## that-agent v0.1.0

The first public release of **that-agent** — the autonomous agent that writes and deploys its own tools.

### Highlights

- **Self-authoring plugins** — the agent writes, installs, upgrades, and removes its own runtime extensions at runtime
- **Cluster-aware fleet** — plugins deployed by any sub-agent are visible across the whole cluster; policy flows downward
- **LLM-judged eval harness** — deterministic scenario runner scores autonomous behavior, not code paths
- **Hot-reload everything** — channels, plugins, and agent identity update at runtime; no restart needed
- **Persistent memory** — SQLite-backed recall that survives restarts and session boundaries
- **Policy-governed tools** — every tool call passes through an Allow / Prompt / Deny gate
- **Sandboxed execution** — Docker and Kubernetes backends; destructive ops denied on host by default
- **Multi-channel routing** — Telegram, HTTP gateway, and TUI through a unified abstraction
- **Heartbeat system** — autonomous listen mode with configurable wakeup cycles and scheduled routines

### Install

**Pre-built binaries** — download from the assets below:

| Platform | Asset |
|---|---|
| macOS (Apple Silicon) | `that-aarch64-apple-darwin.tar.gz` |
| Linux (x86_64) | `that-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (aarch64) | `that-aarch64-unknown-linux-gnu.tar.gz` |

**From crates.io:**

```bash
cargo install that-cli
```

**Docker:**

```bash
docker pull ghcr.io/that-labs/that-agent:v0.1.0
```

### Quickstart

```bash
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env
that run "Create a hello-world Python script and verify it runs"
that chat    # interactive session
```

See the [README](https://github.com/that-labs/that-agent#5-minute-quickstart) for the full quickstart guide.

### Links

- [Architecture](https://github.com/that-labs/that-agent/blob/main/ARCHITECTURE.md)
- [Operator Guide](https://github.com/that-labs/that-agent/blob/main/OPERATORS.md)
- [Contributing](https://github.com/that-labs/that-agent/blob/main/CONTRIBUTING.md)
