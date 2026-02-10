# Document Approval Workflow

**Real-World Use Case**: Contract approval workflow with multiple signers, escalation on timeout, and complete audit trail (DocuSign-like).

## Quick Start

```bash
cd examples/rust/embedded/document_approval
cargo run
```

## What It Demonstrates

1. **Workflow Actor** - Durable multi-step workflow execution
2. **Run Handler** - Main workflow entry point (submit document)
3. **Signal Handlers** - External events (approve, reject, escalate, remind)
4. **Query Handlers** - Read-only queries (status, audit_trail)
5. **Timer Facet** - Auto-escalation after timeout
6. **Audit Trail** - Complete history of all actions

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│               DocumentApprovalWorkflow                          │
├─────────────────────────────────────────────────────────────────┤
│  State: WorkflowState                                           │
│    - document_id, status, current_approver_index               │
│    - decisions[], audit_trail[], escalation_count              │
│                                                                 │
│  Handlers:                                                      │
│    #[run_handler]                → Submit document              │
│    #[signal_handler("approve")]  → Record approval              │
│    #[signal_handler("reject")]   → Record rejection             │
│    #[signal_handler("escalate")] → Escalate to manager          │
│    #[signal_handler("remind")]   → Send reminder                │
│    #[query_handler("status")]    → Get current status           │
│    #[query_handler("audit_trail")] → Get full audit history     │
│                                                                 │
│  Facets:                                                        │
│    TimerFacet → Auto-escalation after timeout                   │
└─────────────────────────────────────────────────────────────────┘
```

## Workflow Flow

```
Submit Document
       │
       ▼
┌──────────────┐     approve     ┌──────────────┐     approve     ┌──────────────┐
│   Approver 1 │ ───────────────▶│   Approver 2 │ ───────────────▶│   Approver 3 │
│ (Legal)      │                 │ (Finance)    │                 │ (Executive)  │
└──────────────┘                 └──────────────┘                 └──────────────┘
       │                                │                                │
       │ timeout                        │ timeout                        │ approve
       ▼                                ▼                                ▼
   Escalate                         Escalate                         Complete
       │                                │                                │
       │ reject                         │ reject                         │
       ▼                                ▼                                │
   Rejected                         Rejected                             │
                                                                         ▼
                                                                    ✓ Approved
```

## SDK Pattern

```rust
use plexspaces_sdk::*;

// 1. Define workflow actor
#[workflow_actor]
struct DocumentApprovalWorkflow {
    state: WorkflowState,
    request: Option<ApprovalRequest>,
}

// 2. Add workflow handlers
#[plexspaces_handlers(workflow)]
impl DocumentApprovalWorkflow {
    // Main workflow entry point
    #[run_handler]
    async fn run(&mut self, ctx: &ActorContext, input: Message) 
        -> Result<Message, BehaviorError> {
        // Parse request, route to first approver
    }
    
    // Handle approval signal
    #[signal_handler("approve")]
    async fn on_approve(&mut self, ctx: &ActorContext, data: Message) 
        -> Result<(), BehaviorError> {
        // Record decision, route to next approver or complete
    }
    
    // Handle rejection signal
    #[signal_handler("reject")]
    async fn on_reject(&mut self, ctx: &ActorContext, data: Message) 
        -> Result<(), BehaviorError> {
        // Record rejection, end workflow
    }
    
    // Query current status
    #[query_handler("status")]
    async fn get_status(&self, ctx: &ActorContext, params: Message) 
        -> Result<Message, BehaviorError> {
        // Return current workflow state
    }
}

// 3. Spawn with timer facet for escalation
let timer_facet = TimerFacet::new(json!({}), 50);
let workflow_ref = spawn_actor(&ctx, service_locator, "approval-001", "contracts",
    DocumentApprovalWorkflow::new(), vec![Box::new(timer_facet)]).await?;

// 4. Submit document (workflow_run message type)
let workflow_id = workflow_ref.id().to_string();
let msg = Message {
    id: ulid::Ulid::new().to_string(),
    receiver_id: workflow_id.clone(),
    message_type: "workflow_run".to_string(),
    payload: serde_json::to_vec(&approval_request)?,
    ..Default::default()
};
workflow_ref.ask(msg, timeout).await?;

// 5. Send approval signal (workflow_signal:approve message type)
let approve_msg = Message {
    id: ulid::Ulid::new().to_string(),
    receiver_id: workflow_id.clone(),
    message_type: "workflow_signal:approve".to_string(),
    payload: serde_json::to_vec(&json!({"approver_id": "alice"}))?,
    ..Default::default()
};
workflow_ref.tell(approve_msg).await?;

// 6. Query status (workflow_query:status message type)
let status_msg = Message {
    id: ulid::Ulid::new().to_string(),
    receiver_id: workflow_id.clone(),
    message_type: "workflow_query:status".to_string(),
    payload: vec![],
    ..Default::default()
};
let status = workflow_ref.ask(status_msg, timeout).await?;
```

## Key APIs

| API | Purpose |
|-----|---------|
| `#[workflow_actor]` | Mark struct as Workflow actor |
| `#[run_handler]` | Main workflow execution entry |
| `#[signal_handler("name")]` | Handle external signals |
| `#[query_handler("name")]` | Handle read-only queries |
| `TimerFacet` | Schedule reminders/escalation |

## Message Types

| Message Type | Purpose |
|--------------|---------|
| `workflow_run` | Start/run the workflow |
| `workflow_signal:<name>` | Send signal to workflow (e.g., `workflow_signal:approve`) |
| `workflow_query:<name>` | Query workflow state (e.g., `workflow_query:status`) |

## Durability

Add durability facet for crash recovery:

```rust
#[workflow_actor(facets = ["durability"])]
struct DurableApprovalWorkflow { ... }
```

With durability:
- Workflow state persisted to journal
- Survives node crashes
- Resumes from last checkpoint

## Use Cases

- Contract approvals (DocuSign, Adobe Sign)
- Expense report approvals
- Purchase order approvals
- Leave/vacation requests
- Code review workflows
- Compliance sign-offs
- Multi-party document signing

## See Also

- [SDK Documentation](../../../../docs/sdk.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
