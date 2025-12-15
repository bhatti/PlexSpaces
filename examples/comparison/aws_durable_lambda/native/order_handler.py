# AWS Durable Lambda Order Handler (Python)
# Demonstrates durable execution patterns with idempotency

import json
from aws_lambda_durable import DurableFunction

@DurableFunction
def process_order(event, context):
    order = json.loads(event['body'])
    order_id = order['order_id']
    
    # Step 1: Validate order (idempotent)
    validation = validate_order(order_id, order)
    if not validation['valid']:
        return {
            'statusCode': 400,
            'body': json.dumps({'status': 'rejected', 'reason': validation['error']})
        }
    
    # Step 2: Process payment (idempotent with idempotency key)
    payment = process_payment(
        order_id=order_id,
        amount=order['amount'],
        idempotency_key=f"payment-{order_id}"
    )
    
    if not payment['success']:
        return {
            'statusCode': 402,
            'body': json.dumps({'status': 'payment_failed', 'reason': payment['error']})
        }
    
    # Step 3: Ship order (idempotent)
    shipment = ship_order(
        order_id=order_id,
        address=order['shipping_address'],
        idempotency_key=f"shipment-{order_id}"
    )
    
    return {
        'statusCode': 200,
        'body': json.dumps({
            'status': 'completed',
            'order_id': order_id,
            'transaction_id': payment['transaction_id'],
            'tracking_number': shipment['tracking_number']
        })
    }

# Lambda handler
def lambda_handler(event, context):
    return process_order(event, context)
