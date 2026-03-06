# Restate native reference: exactly-once payment

This file describes how the same use case is implemented with [Restate](https://restate.dev).

## Restate model

- **Durable execution**: Restate journals every step (side effect). On replay after crash/restart, already-journaled steps are replayed from log; new steps run once.
- **Idempotency**: Handlers can be keyed by idempotency key; duplicate requests with the same key return the cached result (exactly-once).
- **Service**: You implement a service (e.g. `PaymentService`) with methods like `processPayment`. Restate guarantees exactly-once execution per invocation.

## Native pattern (conceptual)

```typescript
// Restate SDK (simplified): service with durable execution
export const paymentService = restate.service({
  name: "PaymentService",
  handlers: {
    async processPayment(ctx: restate.Context, request: PaymentRequest) {
      // ctx.run("validate", () => validate(request));  // journaled
      // ctx.run("debit", () => debit(request.from, request.amount));  // journaled
      // ctx.run("credit", () => credit(request.to, request.amount));   // journaled
      // ctx.run("confirm", () => confirm(request));                   // journaled
      return { status: "confirmed", idempotency_key: request.idempotency_key };
    },
  },
});
```

- Each `ctx.run(name, fn)` is journaled; on replay, completed runs return cached result.
- Idempotency is often handled by using the request id or idempotency_key as the durable execution id.

## PlexSpaces equivalent

- **Durability facet**: Checkpoints actor state (getState/setState) so replay restores state.
- **Idempotency**: We store results keyed by `idempotency_key` in state; duplicate requests return cached result.
- **Workflow run()**: Single entry point; steps (validate → debit → credit → confirm) run in order; state is checkpointed so restart does not double-execute.

See README comparison table (Restate vs PlexSpaces).
