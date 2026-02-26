---
name: channel-notify
description: Explains when and how to proactively notify the human operator during long-running tasks, without pausing to wait for a response.
metadata:
  bootstrap: true
  always: true
  version: 1.0.0
---

# Channel Notify

You have a built-in tool called `channel_notify` that lets you send a one-way message to the
human operator at any point during a task — without interrupting your work or waiting for a reply.

Use it when you want to share something meaningful as you go, rather than revealing everything
only in the final response.

## When to Use It

Use `channel_notify` when:

- You have completed a significant phase of a long-running task and the human would benefit
  from knowing where things stand before you continue.
- You discovered something important — a relevant finding, a risk, or a change in direction —
  that would be good for the human to know about now.
- A task is taking longer than expected and you want to reassure the operator that progress
  is being made.
- You have reached a natural checkpoint before a potentially irreversible action (but note:
  if you actually need approval, use the `human_ask` tool instead).

## When NOT to Use It

Do not use `channel_notify` when:

- You actually need the human to make a decision or give approval — use `human_ask` instead.
- The task is short and the final response will speak for itself.
- The update adds noise without useful information (avoid "still working…" spam).

## Tone and Formatting

Keep notifications concise and specific. A good notification tells the human what happened or
was found, not just that you are working. Follow the active channel's formatting conventions
(visible in the **Active Channels** section of your context).

## Example Intent (not literal script)

When you have finished scanning a large set of inputs and are about to start generating output,
a brief notification like "Finished analysis of N items — starting synthesis now" is helpful.
When you are three minutes into a five-step pipeline and step two just completed successfully,
a quick status ping keeps the human informed without requiring them to wait and wonder.
