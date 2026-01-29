// Restate Order Service (TypeScript)
// Demonstrates durable execution with journaling and deterministic replay

import * as restate from "@restate/sdk";

const orderService = restate.service({
    name: "OrderService",
    handlers: {
        async processOrder(ctx: restate.Context, order: Order): Promise<OrderResult> {
            // Step 1: Validate order (durable, journaled)
            const validation = await ctx.sideEffect(async () => {
                return await validateOrder(order);
            });
            
            if (!validation.valid) {
                return { status: "rejected", reason: validation.error };
            }
            
            // Step 2: Process payment (durable, journaled)
            const payment = await ctx.sideEffect(async () => {
                return await processPayment(order);
            });
            
            if (!payment.success) {
                return { status: "payment_failed", reason: payment.error };
            }
            
            // Step 3: Ship order (durable, journaled)
            const shipment = await ctx.sideEffect(async () => {
                return await shipOrder(order);
            });
            
            return {
                status: "completed",
                orderId: order.id,
                transactionId: payment.transactionId,
                trackingNumber: shipment.trackingNumber
            };
        }
    }
});

// Usage:
// const result = await orderService.processOrder({
//     id: "order-123",
//     amount: 99.99,
//     items: [...]
// });
