// Dapr Unified Durable Workflow (TypeScript)
// Demonstrates unified durable workflows with actors

import { DaprClient, DaprServer } from "@dapr/client";

const daprClient = new DaprClient();

// Define workflow
async function orderWorkflow(order: Order): Promise<OrderResult> {
    // Step 1: Process payment (via actor)
    const paymentActor = await daprClient.actor.get("PaymentActor", order.id);
    const payment = await paymentActor.processPayment(order);
    
    // Step 2: Update inventory (via actor)
    const inventoryActor = await daprClient.actor.get("InventoryActor", order.id);
    const inventory = await inventoryActor.reserveItems(order.items);
    
    // Step 3: Create shipment (via actor)
    const shippingActor = await daprClient.actor.get("ShippingActor", order.id);
    const shipment = await shippingActor.createShipment(order);
    
    return {
        status: "completed",
        orderId: order.id,
        transactionId: payment.transactionId,
        trackingNumber: shipment.trackingNumber
    };
}

// Usage:
// const result = await orderWorkflow({
//     id: "order-123",
//     amount: 99.99,
//     items: [...]
// });
