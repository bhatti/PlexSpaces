// Temporal Order Processing Workflow
// This is sample code showing the native Temporal implementation

import { proxyActivities, log } from '@temporalio/workflow';
import type * as activities from './activities';

const { validateOrder, processPayment, shipOrder, sendConfirmation } = 
    proxyActivities<typeof activities>({
        startToCloseTimeout: '1 minute',
        retry: {
            initialInterval: '1s',
            backoffCoefficient: 2,
            maximumAttempts: 3,
        },
    });

export async function orderWorkflow(orderId: string, order: Order): Promise<void> {
    log.info('Order workflow started', { orderId });

    try {
        // Step 1: Validate order
        const validation = await validateOrder(order);
        if (!validation.valid) {
            throw new Error(`Order validation failed: ${validation.errors.join(', ')}`);
        }

        // Step 2: Process payment
        const payment = await processPayment({
            orderId,
            amount: order.total,
            paymentMethod: order.paymentMethod,
        });

        if (payment.status !== 'completed') {
            throw new Error(`Payment failed: ${payment.error}`);
        }

        // Step 3: Ship order
        const shipment = await shipOrder({
            orderId,
            address: order.shippingAddress,
            items: order.items,
        });

        // Step 4: Send confirmation
        await sendConfirmation({
            orderId,
            email: order.customerEmail,
            trackingNumber: shipment.trackingNumber,
        });

        log.info('Order workflow completed', { orderId });
    } catch (error) {
        log.error('Order workflow failed', { orderId, error });
        throw error;
    }
}

// activities.ts
export async function validateOrder(order: Order): Promise<ValidationResult> {
    // External API call - automatically retried on failure
    const response = await fetch('https://api.example.com/validate', {
        method: 'POST',
        body: JSON.stringify(order),
    });
    return response.json();
}

export async function processPayment(payment: PaymentRequest): Promise<PaymentResult> {
    // Payment processing - idempotent, retried on failure
    const response = await fetch('https://api.payment.com/charge', {
        method: 'POST',
        body: JSON.stringify(payment),
    });
    return response.json();
}

export async function shipOrder(shipment: ShipmentRequest): Promise<ShipmentResult> {
    // Shipping API - retried on failure
    const response = await fetch('https://api.shipping.com/create', {
        method: 'POST',
        body: JSON.stringify(shipment),
    });
    return response.json();
}

export async function sendConfirmation(confirmation: ConfirmationRequest): Promise<void> {
    // Email service - retried on failure
    await fetch('https://api.email.com/send', {
        method: 'POST',
        body: JSON.stringify(confirmation),
    });
}
