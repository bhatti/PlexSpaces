// SPDX-License-Identifier: AGPL-3.0-or-later
// CronSchedulerActor — tick-based scheduler with distributed leader election.
// Demonstrates: SendAfter (tick loop), Channel (job delivery), DistributedLock
// (single-leader guarantee), KV (job definitions + last-run tracking).
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const cronChannel = "cron:pending"

// CronSchedulerActor runs a tick loop via SendAfter to evaluate scheduled jobs.
// A DistributedLock ensures only one node in the cluster processes cron at a time.
// Jobs are delivered via Channel for durable, at-least-once execution by AgentActor.
type CronSchedulerActor struct {
	plexspaces.BaseActor
	TickCount    int    `json:"tick_count"`
	JobCount     int    `json:"job_count"`
	TriggeredCount int  `json:"triggered_count"`
	TickInterval uint64 `json:"tick_interval_ms"`
}

func NewCronSchedulerActor() plexspaces.Actor {
	a := &CronSchedulerActor{TickInterval: 60000}
	a.SetSelf(a)
	return a
}

func newCronSchedulerActor() *CronSchedulerActor {
	a := &CronSchedulerActor{TickInterval: 60000}
	a.SetSelf(a)
	return a
}

func (c *CronSchedulerActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	c.SetRuntimeMetadata(config.ActorID)
	if iv := config.Args["tick_interval_ms"]; iv != "" {
		var n uint64
		if _, err := fmt.Sscan(iv, &n); err == nil && n >= 1000 {
			c.TickInterval = n
		}
	}
	if err := host.PG().Join("svc:cron"); err != nil {
		host.Warn(fmt.Sprintf("CronSchedulerActor: failed to join svc:cron: %v", err))
	}
	// Schedule first tick
	_ = host.SendAfter(c.TickInterval, "tick", map[string]any{"op": "tick"})
	host.Info(fmt.Sprintf("CronSchedulerActor Init actor_id=%s interval_ms=%d", config.ActorID, c.TickInterval))
	return ""
}

func (c *CronSchedulerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "tick":
		return c.tick()
	case "trigger_tick":
		return c.tick() // force tick for testing
	case "create_job":
		return c.createJob(p)
	case "delete_job":
		return c.deleteJob(p)
	case "list_jobs":
		return c.listJobs()
	case "get_job":
		return c.getJob(p)
	case "get_stats":
		depth, _ := host.Ch().Depth("", cronChannel)
		return marshal(map[string]any{
			"status":          "ok",
			"tick_count":      c.TickCount,
			"job_count":       c.JobCount,
			"triggered_count": c.TriggeredCount,
			"tick_interval":   c.TickInterval,
			"queue_depth":     depth,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (c *CronSchedulerActor) createJob(p map[string]any) string {
	jobID := stringVal(p, "job_id", "")
	prompt := stringVal(p, "prompt", "")
	schedule := stringVal(p, "schedule", "every_1h")
	if jobID == "" || prompt == "" {
		return marshal(map[string]any{"error": "job_id and prompt are required"})
	}

	intervalMs := scheduleToMs(schedule)
	job := map[string]any{
		"job_id":      jobID,
		"prompt":      prompt,
		"schedule":    schedule,
		"interval_ms": intervalMs,
		"enabled":     true,
		"created_at":  host.NowMs(),
		"last_run_at": uint64(0),
		"run_count":   0,
	}
	jobJSON, _ := json.Marshal(job)
	host.KVPut("cron_job:"+jobID, string(jobJSON))

	// Track job IDs
	existing := host.KVGet("cron_job_ids")
	ids := []string{}
	if existing != "" {
		ids = strings.Split(existing, ",")
	}
	found := false
	for _, id := range ids {
		if id == jobID {
			found = true
			break
		}
	}
	if !found {
		ids = append(ids, jobID)
		host.KVPut("cron_job_ids", strings.Join(ids, ","))
		c.JobCount++
	}

	fireAudit("cron_job_created", fmt.Sprintf("job_id=%s schedule=%s", jobID, schedule))
	host.Info(fmt.Sprintf("CronSchedulerActor: created job=%s schedule=%s interval_ms=%d", jobID, schedule, intervalMs))
	return marshal(map[string]any{"status": "ok", "job_id": jobID, "schedule": schedule, "interval_ms": intervalMs})
}

func (c *CronSchedulerActor) deleteJob(p map[string]any) string {
	jobID := stringVal(p, "job_id", "")
	if jobID == "" {
		return marshal(map[string]any{"error": "job_id is required"})
	}
	host.KVDelete("cron_job:" + jobID)
	existing := host.KVGet("cron_job_ids")
	if existing != "" {
		ids := strings.Split(existing, ",")
		newIDs := make([]string, 0, len(ids))
		for _, id := range ids {
			if id != jobID {
				newIDs = append(newIDs, id)
			}
		}
		host.KVPut("cron_job_ids", strings.Join(newIDs, ","))
	}
	if c.JobCount > 0 {
		c.JobCount--
	}
	return marshal(map[string]any{"status": "ok", "job_id": jobID})
}

func (c *CronSchedulerActor) listJobs() string {
	existing := host.KVGet("cron_job_ids")
	if existing == "" {
		return marshal(map[string]any{"status": "ok", "jobs": []any{}, "count": 0})
	}
	ids := strings.Split(existing, ",")
	jobs := make([]any, 0, len(ids))
	for _, id := range ids {
		if id == "" {
			continue
		}
		raw := host.KVGet("cron_job:" + id)
		if raw == "" {
			continue
		}
		var job map[string]any
		if err := json.Unmarshal([]byte(raw), &job); err == nil {
			jobs = append(jobs, job)
		}
	}
	return marshal(map[string]any{"status": "ok", "jobs": jobs, "count": len(jobs)})
}

func (c *CronSchedulerActor) getJob(p map[string]any) string {
	jobID := stringVal(p, "job_id", "")
	if jobID == "" {
		return marshal(map[string]any{"error": "job_id is required"})
	}
	raw := host.KVGet("cron_job:" + jobID)
	if raw == "" {
		return marshal(map[string]any{"error": "job not found", "job_id": jobID})
	}
	var job map[string]any
	if err := json.Unmarshal([]byte(raw), &job); err != nil {
		return marshal(map[string]any{"error": "corrupt job data"})
	}
	job["status"] = "ok"
	return marshal(job)
}

// tick is the core scheduling loop. Runs every TickInterval ms via SendAfter.
// Uses DistributedLock to ensure single-leader execution across a cluster.
func (c *CronSchedulerActor) tick() string {
	now := host.NowMs()

	// Try to acquire leader lock (non-blocking: timeoutMs=0)
	holderID := fmt.Sprintf("cron-%d", now)
	lockResult := host.LockAcquire("minihermes", "minihermes", holderID, "cron_leader", 120, 0)
	if lockResult == "" || plexspaces.IsHostError(lockResult) {
		// Another node holds the lock; skip this tick but reschedule
		_ = host.SendAfter(c.TickInterval, "tick", map[string]any{"op": "tick"})
		return marshal(map[string]any{"status": "ok", "leader": false, "tick_count": c.TickCount})
	}
	var lockInfo struct {
		LockKey string `json:"lock_key"`
		Version string `json:"version"`
	}
	_ = json.Unmarshal([]byte(lockResult), &lockInfo)
	defer func() {
		_ = host.LockRelease(lockInfo.LockKey, "minihermes", "minihermes", holderID, lockInfo.Version)
	}()

	c.TickCount++
	triggered := 0

	existing := host.KVGet("cron_job_ids")
	if existing != "" {
		ids := strings.Split(existing, ",")
		for _, id := range ids {
			if id == "" {
				continue
			}
			raw := host.KVGet("cron_job:" + id)
			if raw == "" {
				continue
			}
			var job map[string]any
			if err := json.Unmarshal([]byte(raw), &job); err != nil {
				continue
			}
			if !boolVal(job, "enabled") {
				continue
			}
			intervalMs, _ := job["interval_ms"].(float64)
			lastRunAt, _ := job["last_run_at"].(float64)
			if intervalMs == 0 || (now-uint64(lastRunAt)) < uint64(intervalMs) {
				continue
			}

			// Job is due: send to Channel for AgentActor to process
			jobMsg := map[string]any{
				"job_id":  id,
				"prompt":  stringVal(job, "prompt", ""),
				"run_at":  now,
				"run_num": intVal(job, "run_count", 0) + 1,
			}
			if _, err := host.Ch().Send("", cronChannel, "cron_job", jobMsg); err != nil {
				host.Warn(fmt.Sprintf("CronSchedulerActor: failed to enqueue job %s: %v", id, err))
				continue
			}

			// Update last_run_at and run_count
			job["last_run_at"] = now
			job["run_count"] = intVal(job, "run_count", 0) + 1
			updatedJSON, _ := json.Marshal(job)
			host.KVPut("cron_job:"+id, string(updatedJSON))

			triggered++
			c.TriggeredCount++
			fireAudit("cron_triggered", fmt.Sprintf("job_id=%s", id))
			host.Info(fmt.Sprintf("CronSchedulerActor: triggered job=%s", id))
		}
	}

	// Store health snapshot
	_ = host.TS().Write([]any{"cron_health", now, c.TickCount, triggered})

	// Reschedule next tick
	_ = host.SendAfter(c.TickInterval, "tick", map[string]any{"op": "tick"})

	c.IncrCounter(host, "cron_ticks")
	return marshal(map[string]any{
		"status":    "ok",
		"leader":    true,
		"tick_count": c.TickCount,
		"triggered": triggered,
	})
}

// scheduleToMs converts a human-readable schedule string to milliseconds.
func scheduleToMs(schedule string) uint64 {
	switch schedule {
	case "every_1m":
		return 60 * 1000
	case "every_5m":
		return 5 * 60 * 1000
	case "every_1h":
		return 3600 * 1000
	case "every_24h":
		return 24 * 3600 * 1000
	default:
		return 3600 * 1000
	}
}
