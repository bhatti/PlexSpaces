// Cadence Order Workflow (Go)
// Demonstrates workflow orchestration with activities (Temporal's predecessor)

package main

import (
    "go.uber.org/cadence/workflow"
    "go.uber.org/cadence/activity"
    "context"
)

func OrderWorkflow(ctx workflow.Context, orderID string, amount float64) error {
    ao := workflow.ActivityOptions{
        ScheduleToStartTimeout: time.Minute,
        StartToCloseTimeout:    time.Minute,
    }
    ctx = workflow.WithActivityOptions(ctx, ao)
    
    // Step 1: Execute activity
    var paymentResult string
    err := workflow.ExecuteActivity(ctx, ProcessPayment, orderID, amount).Get(ctx, &paymentResult)
    if err != nil {
        return err
    }
    
    // Step 2: Execute another activity
    var notificationResult string
    err = workflow.ExecuteActivity(ctx, SendNotification, orderID).Get(ctx, &notificationResult)
    if err != nil {
        return err
    }
    
    return nil
}

func ProcessPayment(ctx context.Context, orderID string, amount float64) (string, error) {
    // Activity implementation
    return "payment_completed", nil
}

func SendNotification(ctx context.Context, orderID string) (string, error) {
    // Activity implementation
    return "notification_sent", nil
}
