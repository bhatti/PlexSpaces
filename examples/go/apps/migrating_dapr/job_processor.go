// SPDX-License-Identifier: LGPL-2.1-or-later
// Background Job Processor - Dapr-style (Go WASM)
//
// Durable job queue with retries and dead-letter queue. Workflow behavior +
// virtual_actor + durability; queue/DLQ in actor state (checkpointed).
// Implements plexspaces.WorkflowActor for workflow_run, workflow_signal:cancel, workflow_query:status.
package main

import (
	"encoding/json"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// Job represents a queued or DLQ job.
type Job struct {
	ID       string `json:"id"`
	Payload  string `json:"payload"`
	Retries  int    `json:"retries"`
	MaxRetry int    `json:"max_retry"`
	Status   string `json:"status"` // queued, processing, completed, failed, dlq
	Enqueued uint64 `json:"enqueued_at_ms"`
}

// JobProcessor implements WorkflowActor for Dapr-style background job processing.
type JobProcessor struct {
	plexspaces.BaseActor

	QueueID        string  `json:"queue_id"`
	Queue          []Job   `json:"queue"`
	DLQ            []Job   `json:"dlq"`
	ProcessedCount int     `json:"processed_count"`
	TotalComputeMs float64 `json:"total_compute_ms"`
	TotalCoordMs   float64 `json:"total_coord_ms"`
	CreatedAtMs    uint64  `json:"created_at_ms"`
	UpdatedAtMs    uint64  `json:"updated_at_ms"`
	CancelRequested bool   `json:"cancel_requested"`
}

const processWorkMs = 35
const maxRetries = 3

func NewJobProcessor() *JobProcessor {
	a := &JobProcessor{
		Queue: []Job{},
		DLQ:   []Job{},
	}
	a.SetSelf(a)
	return a
}

func (j *JobProcessor) Init(configJSON string) string {
	var config struct {
		ActorID  string `json:"actor_id"`
		QueueID  string `json:"queue_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	if config.QueueID != "" {
		j.QueueID = config.QueueID
	}
	now := host.NowMs()
	j.CreatedAtMs = now
	j.UpdatedAtMs = now
	return ""
}

func (j *JobProcessor) Run(payloadJSON string) string {
	t0 := host.NowMs()
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	j.UpdatedAtMs = host.NowMs()

	if j.QueueID == "" {
		if q, ok := payload["queue_id"].(string); ok && q != "" {
			j.QueueID = q
		} else {
			j.QueueID = "default"
		}
	}

	action, _ := payload["action"].(string)
	computeMs := 0.0

	switch action {
	case "enqueue":
		jobID, _ := payload["job_id"].(string)
		if jobID == "" {
			return marshal(map[string]any{"error": "job_id required", "queue_id": j.QueueID})
		}
		pl, _ := json.Marshal(payload["payload"])
		j.Queue = append(j.Queue, Job{
			ID:       jobID,
			Payload:  string(pl),
			Retries:  0,
			MaxRetry: maxRetries,
			Status:   "queued",
			Enqueued: host.NowMs(),
		})
		j.UpdatedAtMs = host.NowMs()
		return j.finishRun(t0, computeMs, "enqueued", jobID, len(j.Queue), len(j.DLQ))

	case "process":
		if j.CancelRequested {
			return j.finishRun(t0, 0, "cancelled", "", len(j.Queue), len(j.DLQ))
		}
		if len(j.Queue) == 0 {
			return j.finishRun(t0, 0, "idle", "", 0, len(j.DLQ))
		}
		job := j.Queue[0]
		j.Queue = j.Queue[1:]
		job.Status = "processing"
		computeMs += processWorkMs
		job.Retries++
		// Simulate failure: retry (re-queue) or move to DLQ after maxRetries
		if job.MaxRetry == 0 {
			job.MaxRetry = maxRetries
		}
		if job.Retries < job.MaxRetry && simulateFailure(job.ID, job.Retries) {
			j.Queue = append(j.Queue, job)
			j.UpdatedAtMs = host.NowMs()
			return j.finishRun(t0, computeMs, "retry", job.ID, len(j.Queue), len(j.DLQ))
		}
		if job.Retries >= job.MaxRetry {
			job.Status = "failed"
			j.DLQ = append(j.DLQ, job)
			j.UpdatedAtMs = host.NowMs()
			return j.finishRun(t0, computeMs, "to_dlq", job.ID, len(j.Queue), len(j.DLQ))
		}
		job.Status = "completed"
		j.ProcessedCount++
		j.UpdatedAtMs = host.NowMs()
		return j.finishRun(t0, computeMs, "completed", job.ID, len(j.Queue), len(j.DLQ))
	}

	// Default: return status
	return j.finishRun(t0, 0, "ok", "", len(j.Queue), len(j.DLQ))
}

func (j *JobProcessor) finishRun(t0 uint64, computeMs float64, status, jobID string, queueLen, dlqLen int) string {
	elapsed := float64(host.NowMs() - t0)
	if elapsed < computeMs {
		elapsed = computeMs
	}
	coordMs := elapsed - computeMs
	if coordMs < 0 {
		coordMs = 0
	}
	j.TotalComputeMs += computeMs
	j.TotalCoordMs += coordMs
	return marshal(map[string]any{
		"status":            status,
		"queue_id":          j.QueueID,
		"job_id":            jobID,
		"queue_depth":      queueLen,
		"dlq_size":         dlqLen,
		"processed_count":  j.ProcessedCount,
		"total_compute_ms": j.TotalComputeMs,
		"total_coord_ms":   j.TotalCoordMs,
	})
}

func (j *JobProcessor) Signal(name, _ string) {
	if name == "cancel" {
		j.CancelRequested = true
		j.UpdatedAtMs = host.NowMs()
	}
}

func (j *JobProcessor) Query(name, _ string) string {
	switch name {
	case "status":
		return marshal(map[string]any{
			"queue_id":          j.QueueID,
			"queue_depth":       len(j.Queue),
			"dlq_size":          len(j.DLQ),
			"processed_count":   j.ProcessedCount,
			"cancel_requested": j.CancelRequested,
			"total_compute_ms":  j.TotalComputeMs,
			"total_coord_ms":    j.TotalCoordMs,
			"created_at_ms":    j.CreatedAtMs,
			"updated_at_ms":    j.UpdatedAtMs,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

func (j *JobProcessor) Handle(from, msgType, payload string) string {
	return `{"error":"use workflow_run / workflow_signal / workflow_query"}`
}

// simulateFailure returns true to simulate transient failure (retry); ~25% on first attempt.
func simulateFailure(jobID string, attempt int) bool {
	h := uint32(0)
	for _, c := range jobID {
		h = h*31 + uint32(c)
	}
	return (h%4 == 0) && attempt == 1
}

func marshal(v map[string]any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func init() {
	plexspaces.Register(NewJobProcessor())
}

func main() {}
