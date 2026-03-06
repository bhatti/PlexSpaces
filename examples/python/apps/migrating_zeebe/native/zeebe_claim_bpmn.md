# Zeebe/Camunda native reference: insurance claim workflow

This file describes how the same use case is implemented with [Zeebe](https://zeebe.io) or [Camunda](https://camunda.com).

## Zeebe/Camunda model

- **BPMN**: Workflows are defined as BPMN 2.0 (XML or modeler). Tasks can be user tasks (human), service tasks (automated), or timers.
- **Timers**: Boundary timers or intermediate timer events implement SLA and escalation (e.g. “escalate if not completed in 60s”).
- **Workers**: Service tasks are handled by workers (poll or push); user tasks are completed via task list API.
- **Durability**: Engine state is persisted; workflows survive restarts.

## Native pattern (conceptual)

```xml
<!-- BPMN: claim process (simplified) -->
<process id="claim-process">
  <startEvent id="start"/>
  <sequenceFlow sourceRef="start" targetRef="validate"/>
  <serviceTask id="validate" name="Validate Claim"/>
  <sequenceFlow sourceRef="validate" targetRef="human-review"/>
  <userTask id="human-review" name="Review Claim">
    <boundaryEvent id="sla-timer" cancelActivity="false" attachedToRef="human-review">
      <timerEventDefinition><timeDuration>PT60S</timeDuration></timerEventDefinition>
    </boundaryEvent>
  </userTask>
  <sequenceFlow sourceRef="human-review" targetRef="gateway"/>
  <exclusiveGateway id="gateway"/>
  <sequenceFlow sourceRef="gateway" targetRef="approved" name="approve"/>
  <sequenceFlow sourceRef="gateway" targetRef="rejected" name="reject"/>
  <sequenceFlow sourceRef="sla-timer" targetRef="escalated"/>
  ...
</process>
```

- Workers complete service tasks; user tasks are completed via REST/API with outcome (approve/reject).
- Timer boundary event fires after 60s and flows to “escalated”.

## PlexSpaces equivalent

- **Workflow actor**: Single actor per claim (virtual_actor); `run()` advances steps: submit → validate → pending_review; payload `action=approve|reject` or SLA/escalation completes the flow.
- **Reminder facet**: Attached for durable SLA reminders; when the host fires `ReminderFired`, the actor’s `signal("ReminderFired", data)` sets escalation and the next `run()` marks the claim escalated.
- **Durability**: State checkpointed so workflow survives restarts.

See README comparison table (Zeebe vs PlexSpaces).
