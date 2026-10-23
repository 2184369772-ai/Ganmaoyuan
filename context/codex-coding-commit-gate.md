# Keep the Why - Ganmaoyuan

## Evidence required to accept an embedded Codex coding task

**Type:** user decision / acceptance constraint
**Status:** active
**Evidence:** confirmed
**Source:** DecisionRecord: 9955c136-8882-43c7-81e5-28b54323b439; user message: e8878f61-fc9f-48b4-9519-7155f21afb3b
**Verification:** Git: 77998d1ca4e64d4ba662bd415666a1a9c9da289c (`fix: require safe commits for coding tasks`)
**Revisit when:** No review condition was recorded in the source decision.

**Applies to:** Coding tasks executed by Ganmaoyuan's embedded Codex runner in the SelfProject. This entry does not claim a rule for other projects or non-coding tasks.

**Reason:** The source message explicitly makes verifiable task-linked Git commit and Result evidence an acceptance condition. It records no deeper motivation; none is inferred here.

**Recorded constraints:**
- The Git commit and Result evidence must belong to this task and be verifiable before acceptance.
- Changes that already existed before the task must not be included in its commit.
- A task must not be represented as complete when its evidence is missing.
