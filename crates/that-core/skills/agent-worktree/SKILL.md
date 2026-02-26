---
name: agent-worktree
description: Coordinate multiple agents working on the same repository using isolated git worktree branches. Use when orchestrating parallel agent work, reviewing changes from child agents, or merging completed work.
metadata:
  bootstrap: true
  version: 1.0.0
---

# Agent Worktree Orchestration

This skill describes how to use git worktrees for safe, concurrent multi-agent collaboration
on a shared repository. Each agent works on an isolated branch — no merge conflicts during
active work, and the orchestrating agent reviews and merges when ready.

## Core Workflow

### Setting Up Isolated Work

When delegating a task to another agent (or working on a parallel track yourself):

1. **Create a worktree** for the agent using `worktree_create` with the repository root and agent name
2. The tool creates a timestamped branch and a dedicated working directory
3. Direct the agent to work exclusively within its worktree path
4. The agent commits changes normally — they stay on its isolated branch

### Reviewing Work

Before merging, review what an agent has done:

1. **Check the diff** with `worktree_diff` to see all changes since the branch diverged
2. **Check the log** with `worktree_log` to see the commit history
3. If changes need revision, communicate with the agent and let it continue working

### Merging Completed Work

When an agent's work is ready:

1. **Merge** with `worktree_merge` — this creates a no-fast-forward merge into the target branch
2. If conflicts occur, the merge is aborted and conflict files are reported
3. After successful merge, **clean up** with `worktree_discard` to remove the worktree

### Listing Active Worktrees

Use `worktree_list` to see all active agent worktrees, their branches, and paths.

## Multi-Agent Orchestration Pattern

When orchestrating multiple agents on the same project:

1. **Create one worktree per agent** — each agent gets its own isolated branch
2. **Assign tasks** — each agent works within its worktree directory
3. **Review incrementally** — check diffs as agents report progress
4. **Merge in order** — merge completed work one agent at a time to manage conflicts
5. **Clean up** — discard worktrees after merging

## Guiding Principles

**Isolation first.** Never have two agents commit to the same branch simultaneously.
Worktrees enforce this naturally — each agent has its own branch.

**Review before merge.** Always check the diff and log before merging. Automated agents
can produce unexpected changes.

**Merge sequentially.** When multiple agents finish, merge one at a time. This keeps
conflict resolution manageable.

**Clean up after yourself.** Discard worktrees once their branches are merged. Stale
worktrees consume disk space and can cause confusion.
