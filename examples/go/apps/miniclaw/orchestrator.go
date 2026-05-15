// SPDX-License-Identifier: AGPL-3.0-or-later
// OrchestratorActor — durable workflow that decomposes tasks and delegates to agents.
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// OrchestratorActor is a durable workflow that decomposes a task into sub-tasks,
// delegates each to an AgentActor discovered via process group, and aggregates
// results via TupleSpace coordination.
//
// The framework routes workflow_run/workflow_signal/workflow_query to Run/Signal/Query
// automatically. Handle() must not dispatch these itself.
type OrchestratorActor struct {
	plexspaces.BaseActor
	Status   string `json:"status"`
	TaskID   string `json:"task_id"`
	Progress int    `json:"progress"`
}

func NewOrchestratorActor() plexspaces.Actor {
	a := &OrchestratorActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func newOrchestratorActor() *OrchestratorActor {
	a := &OrchestratorActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (o *OrchestratorActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	o.SetRuntimeMetadata(config.ActorID)
	o.Status = "idle"
	host.Info(fmt.Sprintf("OrchestratorActor Init actor_id=%s", config.ActorID))
	return ""
}

// Handle must NOT dispatch workflow_run/signal/query — the framework does that.
func (o *OrchestratorActor) Handle(from, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "workflow_run":
		return o.Run(payloadJSON)
	case "workflow_query":
		name := stringVal(p, "name", "status")
		return o.Query(name, payloadJSON)
	case "workflow_signal":
		name := stringVal(p, "name", "")
		o.Signal(name, payloadJSON)
		return marshal(map[string]any{"ok": true})
	}
	return marshal(map[string]any{"error": "use workflow_run / workflow_signal / workflow_query"})
}

func (o *OrchestratorActor) Run(payloadJSON string) string {
	p := parsePayload(payloadJSON)
	task := stringVal(p, "task", "explain how agents work")
	taskID := stringVal(p, "task_id", fmt.Sprintf("orch-%d", host.NowMs()))

	o.Status = "running"
	o.TaskID = taskID
	o.Progress = 0

	host.Info(fmt.Sprintf("OrchestratorActor Run taskID=%s task=%s", taskID, task))

	agentID, err := pgFirst("svc:agent")
	if err != nil {
		o.Status = "failed"
		return marshal(map[string]any{"error": "no agents in svc:agent process group", "task_id": taskID})
	}

	// Decompose task into sub-tasks by splitting on " and ".
	subTasks := []string{}
	if idx := strings.Index(strings.ToLower(task), " and "); idx >= 0 {
		subTasks = append(subTasks, strings.TrimSpace(task[:idx]))
		subTasks = append(subTasks, strings.TrimSpace(task[idx+5:]))
	} else {
		subTasks = []string{task}
	}

	subResults := make([]any, 0, len(subTasks))
	for i, subTask := range subTasks {
		o.Progress = (i + 1) * 100 / len(subTasks)
		resp, err := host.Ask(agentID, "chat", map[string]any{
			"op":         "chat",
			"message":    subTask,
			"session_id": fmt.Sprintf("orch-%s-%d", taskID, i),
		}, 15000)
		if err != nil {
			o.Status = "failed"
			return marshal(map[string]any{"error": "sub-task failed: " + err.Error(), "task_id": taskID})
		}
		resultJSON, _ := json.Marshal(resp)
		_ = host.TS().Write([]any{"orch_result", taskID, i, string(resultJSON)})
		subResults = append(subResults, resp)
	}

	summaries := make([]string, 0, len(subResults))
	for _, r := range subResults {
		if rm, ok := r.(map[string]any); ok {
			if response := stringVal(rm, "response", ""); response != "" {
				summaries = append(summaries, response)
			}
		}
	}
	aggregated := strings.Join(summaries, " | ")

	o.Status = "completed"
	o.Progress = 100

	o.IncrCounter(host, "orchestrator_runs")
	fireAudit("orchestrator_completed", fmt.Sprintf("task_id=%s subtasks=%d", taskID, len(subTasks)))
	return marshal(map[string]any{
		"status":      "ok",
		"task_id":     taskID,
		"result":      aggregated,
		"sub_results": subResults,
		"sub_tasks":   len(subTasks),
	})
}

func (o *OrchestratorActor) Signal(name, payloadJSON string) {
	switch name {
	case "cancel":
		o.Status = "cancelled"
		host.Info(fmt.Sprintf("OrchestratorActor cancelled task_id=%s", o.TaskID))
	}
}

func (o *OrchestratorActor) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"task_id":  o.TaskID,
			"status":   o.Status,
			"progress": o.Progress,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}
