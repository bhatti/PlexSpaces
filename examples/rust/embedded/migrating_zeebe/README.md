# Zeebe vs PlexSpaces Comparison

This comparison demonstrates how to implement Zeebe-style BPMN workflow engine with event sourcing for loan approval in both Zeebe and PlexSpaces.

## Use Case: Loan Approval Workflow with Event Sourcing

A BPMN workflow system that:
- Processes loan applications through multiple steps
- Performs credit checks and risk assessments
- Makes approval decisions (gateway)
- Disburses approved loans or rejects applications
- Records all events in immutable event log (event sourcing)

**Workflow Steps**: Application → Credit Check → Risk Assessment → Approval Decision (Gateway) → Disburse/Reject

---

## Zeebe Implementation

### Native Java Code

```java
// LoanApprovalWorkflow.java
import io.zeebe.client.ZeebeClient;
import io.zeebe.client.api.response.WorkflowInstanceEvent;

ZeebeClient client = ZeebeClient.newClientBuilder()
    .brokerContactPoint("localhost:26500")
    .build();

// Deploy workflow (BPMN)
client.newDeployCommand()
    .addResourceFile("loan-approval.bpmn")
    .send()
    .join();

// Start workflow instance (event sourcing)
WorkflowInstanceEvent workflowInstance = client
    .newCreateInstanceCommand()
    .bpmnProcessId("loan-approval")
    .latestVersion()
    .variables(Map.of(
        "applicationId", "loan-app-001",
        "applicantName", "John Doe",
        "loanAmount", 50000.0,
        "creditScore", 720,
        "annualIncome", 150000.0
    ))
    .send()
    .join();

// Events are automatically recorded (event sourcing)
// - WORKFLOW_STARTED
// - STEP_COMPLETED (SubmitApplication)
// - STEP_COMPLETED (CreditCheck)
// - STEP_COMPLETED (RiskAssessment)
// - STEP_COMPLETED (ApprovalDecision)
// - STEP_COMPLETED (DisburseLoan or RejectApplication)
// - WORKFLOW_COMPLETED
```

### BPMN Workflow Definition

```xml
<bpmn:process id="loan-approval" name="Loan Approval Workflow">
  <bpmn:startEvent id="start" name="Start"/>
  
  <bpmn:serviceTask id="submit-application" name="Submit Application"/>
  <bpmn:serviceTask id="credit-check" name="Credit Check"/>
  <bpmn:serviceTask id="risk-assessment" name="Risk Assessment"/>
  
  <bpmn:exclusiveGateway id="approval-decision" name="Approval Decision">
    <bpmn:outgoing>approved</bpmn:outgoing>
    <bpmn:outgoing>rejected</bpmn:outgoing>
  </bpmn:exclusiveGateway>
  
  <bpmn:serviceTask id="disburse-loan" name="Disburse Loan"/>
  <bpmn:serviceTask id="reject-application" name="Reject Application"/>
  
  <bpmn:endEvent id="end" name="End"/>
</bpmn:process>
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// BPMN workflow with event sourcing (Zeebe pattern)
let application = LoanApplication {
    application_id: "loan-app-001".to_string(),
    applicant_name: "John Doe".to_string(),
    loan_amount: 50000.0,
    credit_score: 720,
    employment_status: "employed".to_string(),
    annual_income: 150000.0,
};

// Start workflow instance (events are recorded)
workflow.ask(StartInstance {
    workflow_key: "loan-approval-workflow".to_string(),
    application,
}).await?;

// Event log contains:
// - WORKFLOW_STARTED
// - STEP_COMPLETED (SubmitApplication)
// - STEP_COMPLETED (CreditCheck)
// - STEP_COMPLETED (RiskAssessment)
// - STEP_COMPLETED (ApprovalDecision)
// - STEP_COMPLETED (DisburseLoan or RejectApplication)
// - WORKFLOW_COMPLETED
```

---

## Side-by-Side Comparison

| Feature | Zeebe | PlexSpaces |
|---------|-------|------------|
| **BPMN** | Built-in | WorkflowBehavior |
| **Event Sourcing** | Built-in | DurabilityFacet + Event log |
| **Workflow Steps** | BPMN tasks | Workflow steps |
| **Gateways** | BPMN gateways | Conditional logic |
| **Event Log** | Immutable | Immutable event history |
| **Durability** | Event log | DurabilityFacet |

---

## Key Features

### BPMN Workflow Engine
- **BPMN 2.0**: Standard workflow modeling
- **Service Tasks**: Execute business logic
- **Gateways**: Conditional routing (exclusive, parallel, etc.)
- **Event Sourcing**: All state changes recorded as events

### Event Sourcing
- **Immutable Events**: All workflow state changes recorded
- **Event Replay**: Can replay workflow from event log
- **Audit Trail**: Complete history of workflow execution
- **Time Travel**: Debug by replaying events

### Workflow Steps
- **SubmitApplication**: Initial step
- **CreditCheck**: Check applicant credit score
- **RiskAssessment**: Assess loan risk
- **ApprovalDecision**: Gateway (approve or reject)
- **DisburseLoan**: Execute for approved loans
- **RejectApplication**: Execute for rejected loans

---

## Running the Comparison

```bash
cd examples/comparison/zeebe
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Zeebe Documentation](https://docs.camunda.io/docs/components/zeebe/)
- [Zeebe vs Camunda BPM](https://camunda.com/blog/2019/07/zeebe-workflow-reinvented-for-cloud-and-microservices/)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
