---
name: jira-microtask-breaker
description: Breaks down complex implementation plans or bug reports into granular, Jira-style microtasks (Epics/Bugs) separated by Frontend and Backend to minimize token consumption. Use this skill when a task involves multiple files or layers and needs efficient context management.
---

# Jira Microtask Breaker

This skill enables Gemini CLI to operate as a high-precision execution engine by breaking down large tasks into atomic microtasks. This "Micro-Surgical" approach is mandatory for complex features to prevent context window saturation.

## Core Mandates

1. **Classification**:
   - **Feature/Refactor** -> Classify as **EPIC**.
   - **Bug/Correction** -> Classify as **BUG**.
2. **Layer Isolation**: Strictly separate **Frontend** tasks from **Backend** tasks. A single microtask MUST NOT modify both layers.
3. **Atomicity**: Each microtask should target a specific file or a very small group of highly related files.
4. **Token Hygiene**: Execute exactly ONE microtask per conversational loop. After finishing a task, the agent should ideally start a new turn with a fresh, clean context of the next task.
5. **Persistence**: Use a `JIRA_BOARD.md` file in the feature's `.spec/` directory to track progress.

## Workflow

### Phase 1: The Breakdown (Discovery)
When a request is received, instead of starting the implementation, use this skill to generate the granular board.

1. Analyze the `SPEC.md` and `PLAN.md`.
2. Map out every required change.
3. Generate the board using the `assets/JIRA_BOARD_TEMPLATE.md`.
4. Present the board to the user for approval.

### Phase 2: Micro-Execution
For each task on the board:

1. **State Goal**: Clearly state which Task Key (e.g., `TR-BE-01`) is being executed.
2. **Surgical Read**: Read ONLY the files listed in the task scope.
3. **Execution**: Perform the change.
4. **Validation**: Run unit tests or local checks for that specific task.
5. **Update Board**: Mark the task as done in `JIRA_BOARD.md`.

## Microtask Naming Convention
- `[PROJECT_KEY]-[LAYER]-[ID]`
- Layers: `BE` (Backend), `FE` (Frontend), `VAL` (Validation/E2E).
- Example: `TR-BE-05` (Transfer Backend task 5).

## Example: Bugfix Breakdown
Request: "Fix the transfer button and the balance validation in the service."

1. **Classification**: BUG.
2. **Breakdown**:
   - `TR-BUG-BE-01`: Fix balance validation logic in `TransferTransactionServiceImpl.java`.
   - `TR-BUG-FE-01`: Fix 'Transfer' button state/disabling in `TransferForm.vue`.
   - `TR-BUG-VAL-01`: Verify both fixes in a single flow.

## Resources

### assets/
- **JIRA_BOARD_TEMPLATE.md**: Standard template for tracking the Epic/Bug execution.
