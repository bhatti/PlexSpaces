// Azure Durable Functions Order Orchestration (TypeScript)
// Demonstrates serverless workflow orchestration

import * as df from "durable-functions";

const orchestrator = df.orchestrator(function* (context) {
    const order = context.df.getInput();
    
    // Step 1: Validate order
    const validation = yield context.df.callActivity("ValidateOrder", order);
    if (!validation.valid) {
        return { status: "rejected", reason: validation.error };
    }
    
    // Step 2: Process payment
    const payment = yield context.df.callActivity("ProcessPayment", {
        orderId: order.id,
        amount: order.amount
    });
    
    if (!payment.success) {
        return { status: "payment_failed", reason: payment.error };
    }
    
    // Step 3: Ship order
    const shipment = yield context.df.callActivity("ShipOrder", {
        orderId: order.id,
        address: order.shippingAddress
    });
    
    return {
        status: "completed",
        orderId: order.id,
        transactionId: payment.transactionId,
        trackingNumber: shipment.trackingNumber
    };
});

export default orchestrator;
