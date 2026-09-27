---
name: context-compressor
description: Compresses conversation history and synthesizes current project state to optimize token usage. Use when the context window is large, after completing a major task, or when the user explicitly requests to save tokens or compact the history.
---

# Context Compressor

## Overview
This skill provides a structured way to reduce the token footprint of a conversation by synthesizing the "state of the union". It transforms a verbose history of tool calls and dialogue into a concise "Checkpoint" that allows Gemini CLI to continue working with full context but minimal token overhead.

## Workflow: State Synthesis

When triggered, follow these steps:

### 1. Identify Key State Pillars
Analyze the conversation to extract:
- **Major Decisions:** Architectural choices and rationale.
- **Completed Work:** Files modified and functionality verified.
- **Pending Tasks:** The immediate roadmap.
- **Critical Constraints:** Rules from `GEMINI.md` or `MEMORY.md` that must persist.

### 2. Generate the Consolidated Context Block
Use the templates in [references/compression-patterns.md](references/compression-patterns.md) to create a structured summary. 

**Rules for Synthesis:**
- **No Snippets:** Do not include large blocks of code.
- **No Logs:** Exclude stack traces, test outputs, or grep results.
- **Paths First:** Always use absolute or project-relative paths.
- **Impact-Oriented:** Focus on *what changed* and *why*, not the sequence of tool calls.

### 3. Finalize and Handover
Present the summary to the user. Explain that this summary contains everything needed to continue. 

**Pro-Tip:** If the user is on a long-running session, suggest they copy this summary and start a fresh session (`/reset` or a new CLI instance) to immediately reclaim the context window.

## Usage Patterns

### Pattern A: Task Completion
"I've finished the Transfer feature. Let's compress the context."
-> Run the synthesis and provide the "🏁 SESSION CHECKPOINT".

### Pattern B: Token Pressure
"We are running out of tokens. Can you summarize what we've done?"
-> Identify the most relevant decisions and current task state, then provide a condensed view.

## References
- [compression-patterns.md](references/compression-patterns.md): Detailed templates for checkpoints and decision journals.
