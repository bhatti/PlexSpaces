# Insurance Claim Workflow (Python WASM) – Zeebe/Camunda-style

BPMN-style **insurance claim** workflow: submit → validate → (human) review → approve/reject, with **SLA escalation**. Uses **Workflow** behavior plus **virtual_actor**, **durability**, and **reminder** facets.

## Purpose

- **WorkflowActor**: `run(payload)` advances the claim (submit → validate → pending_review; then `action=approve|reject` or escalation).
- **Reminder facet**: Durable SLA reminders; when the host fires `ReminderFired`, the actor escalates on next run.
- **Signal**: `workflow_signal:escalate` to force escalation; `workflow_signal:cancel` to cancel.
- **Query**: `workflow_query:status` for claim status and metrics.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/python/apps/migrating_zeebe
./build.sh
./test.sh 8091
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/claim:{id}` with `{"op":"workflow_run","claim_id":"...", "action":"approve"|"reject"}` (optional). First run = submit, second = validate, third+ = review outcome or escalation check.
- **Signal**: `{"op":"workflow_signal:escalate"}` or `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Native (Zeebe/Camunda) reference

Zeebe/Camunda use BPMN with service tasks, user tasks, and boundary timers for SLA. See **`native/zeebe_claim_bpmn.md`** for a short BPMN sketch and timer-based escalation. This example provides the PlexSpaces equivalent with WorkflowActor + reminder facet + time-based and signal-based escalation.

## Comparison: Zeebe/Camunda vs PlexSpaces

| Feature       | Zeebe/Camunda              | PlexSpaces Python                 |
|---------------|----------------------------|-----------------------------------|
| Model         | BPMN (XML)                 | WorkflowActor run/signal/query    |
| Human task    | User task + task list API  | `action=approve|reject` in run    |
| SLA / timer   | Boundary timer event       | Reminder facet + time check       |
| Durability    | Engine state               | Durability facet (checkpoint)     |
| Escalation    | Timer → escalation flow    | ReminderFired → escalate; or signal |

## References

- [PLAN.md – migrating_zeebe](../../../../PLAN.md)
- [Zeebe](https://zeebe.io), [Camunda](https://camunda.com)
