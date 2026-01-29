// Zeebe Loan Approval Workflow (Java)
// Demonstrates BPMN workflow with event sourcing

import io.zeebe.client.ZeebeClient;
import io.zeebe.client.api.response.WorkflowInstanceEvent;
import java.util.Map;
import java.util.HashMap;

public class LoanApprovalWorkflow {
    public static void main(String[] args) {
        ZeebeClient client = ZeebeClient.newClientBuilder()
            .brokerContactPoint("localhost:26500")
            .build();

        // Deploy workflow (BPMN)
        client.newDeployCommand()
            .addResourceFile("loan-approval.bpmn")
            .send()
            .join();

        // Start workflow instance (event sourcing)
        Map<String, Object> variables = new HashMap<>();
        variables.put("applicationId", "loan-app-001");
        variables.put("applicantName", "John Doe");
        variables.put("loanAmount", 50000.0);
        variables.put("creditScore", 720);
        variables.put("annualIncome", 150000.0);

        WorkflowInstanceEvent workflowInstance = client
            .newCreateInstanceCommand()
            .bpmnProcessId("loan-approval")
            .latestVersion()
            .variables(variables)
            .send()
            .join();

        System.out.println("Workflow instance started: " + workflowInstance.getWorkflowInstanceKey());
        
        // Events are automatically recorded (event sourcing):
        // - WORKFLOW_STARTED
        // - STEP_COMPLETED (SubmitApplication)
        // - STEP_COMPLETED (CreditCheck)
        // - STEP_COMPLETED (RiskAssessment)
        // - STEP_COMPLETED (ApprovalDecision)
        // - STEP_COMPLETED (DisburseLoan or RejectApplication)
        // - WORKFLOW_COMPLETED
    }
}
