// SPDX-License-Identifier: LGPL-2.1-or-later
// Reference: Temporal TypeScript workflow (order fulfillment)
//
// This is a REFERENCE ONLY showing how Temporal workflows look in TypeScript.
// PlexSpaces equivalent: ../order_fulfillment_actor.ts (WorkflowActor with run/signal/query).

/**
 * Temporal order fulfillment workflow (native SDK pattern).
 *
 * In Temporal you use @temporalio/workflow and define a workflow that
 * executes activities (validate, reserve inventory, charge, ship) with
 * signals (cancel) and queries (status).
 *
 * ```ts
 * import { proxyActivities, defineQuery, setHandler } from '@temporalio/workflow';
 * import type * as activities from './activities';
 *
 * const { validateOrder, reserveInventory, chargePayment, shipOrder } =
 *   proxyActivities<typeof activities>({ startToCloseTimeout: '1m' });
 *
 * export const getOrderStatus = defineQuery<string>('getOrderStatus');
 * export const cancelOrder = defineSignal('cancelOrder');
 *
 * export async function orderFulfillmentWorkflow(orderId: string, customerId: string): Promise<string> {
 *   let status = 'pending';
 *   let cancelled = false;
 *
 *   setHandler(cancelOrder, () => { cancelled = true; });
 *   setHandler(getOrderStatus, () => status);
 *
 *   if (cancelled) return 'cancelled';
 *   await validateOrder(orderId);
 *   status = 'validated';
 *
 *   if (cancelled) return compensate();
 *   await reserveInventory(orderId);
 *   status = 'inventory_reserved';
 *
 *   if (cancelled) return compensate();
 *   await chargePayment(orderId, customerId);
 *   status = 'payment_charged';
 *
 *   if (cancelled) return compensate();
 *   await shipOrder(orderId);
 *   status = 'shipped';
 *
 *   return status;
 * }
 * ```
 *
 * PlexSpaces equivalent (../order_fulfillment_actor.ts):
 * - Same run/signal/query pattern: run() = main execution, signal('cancel'), query('status').
 * - WorkflowActor<OrderFulfillmentState> with run(), signal(), query().
 * - Message types: workflow_run, workflow_signal:cancel, workflow_query:status.
 * - Durability via getState/setState (DurabilityFacet in app-config.toml).
 */

export const REFERENCE_ONLY = true;
