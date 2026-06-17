// SPDX-License-Identifier: AGPL-3.0-or-later
// SkillStoreActor — procedural memory that learns from experience.
// Demonstrates: KV (skill metadata), TupleSpace (tag index + pattern matching),
// BlobStorage (procedure bodies), Increment (usage counters), SendAfter (lifecycle).
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// Skill lifecycle states
const (
	skillActive   = "active"
	skillStale    = "stale"
	skillArchived = "archived"
)

// SkillStoreActor stores and retrieves learned procedures (skills).
// Skills are created from multi-step conversations, retrieved by pattern matching,
// and automatically lifecycle-managed (active → stale → archived).
type SkillStoreActor struct {
	plexspaces.BaseActor
	SkillCount    int `json:"skill_count"`
	MatchCount    int `json:"match_count"`
	LearnCount    int `json:"learn_count"`
	ArchiveCount  int `json:"archive_count"`
}

func NewSkillStoreActor() plexspaces.Actor {
	a := &SkillStoreActor{}
	a.SetSelf(a)
	return a
}

func newSkillStoreActor() *SkillStoreActor {
	a := &SkillStoreActor{}
	a.SetSelf(a)
	return a
}

func (s *SkillStoreActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:skills"); err != nil {
		host.Warn(fmt.Sprintf("SkillStoreActor: failed to join svc:skills: %v", err))
	}
	// Schedule daily skill maintenance
	_ = host.SendAfter(86400000, "maintenance_tick", map[string]any{"op": "maintenance_tick"})
	host.Info(fmt.Sprintf("SkillStoreActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *SkillStoreActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "propose_skill":
		return s.proposeSkill(p)
	case "match_skills":
		return s.matchSkills(p)
	case "list_skills":
		return s.listSkills(p)
	case "get_skill":
		return s.getSkill(p)
	case "update_skill":
		return s.updateSkill(p)
	case "delete_skill":
		return s.deleteSkill(p)
	case "evaluate_for_learning":
		return s.evaluateForLearning(p)
	case "record_usage":
		return s.recordUsage(p)
	case "maintenance_tick":
		return s.maintenanceTick()
	case "get_stats":
		return marshal(map[string]any{
			"status":        "ok",
			"skill_count":   s.SkillCount,
			"match_count":   s.MatchCount,
			"learn_count":   s.LearnCount,
			"archive_count": s.ArchiveCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *SkillStoreActor) proposeSkill(p map[string]any) string {
	name := stringVal(p, "name", "")
	description := stringVal(p, "description", "")
	procedure := stringVal(p, "procedure", "")
	tagsRaw := stringVal(p, "tags", "")
	triggersRaw := stringVal(p, "trigger_patterns", "")
	sessionID := stringVal(p, "session_id", "")

	if name == "" || procedure == "" {
		return marshal(map[string]any{"error": "name and procedure are required"})
	}

	skillID := fmt.Sprintf("skill-%d", host.NowMs())
	tags := []string{}
	if tagsRaw != "" {
		for _, t := range strings.Split(tagsRaw, ",") {
			if t = strings.TrimSpace(t); t != "" {
				tags = append(tags, t)
			}
		}
	}
	triggers := []string{}
	if triggersRaw != "" {
		for _, t := range strings.Split(triggersRaw, ",") {
			if t = strings.TrimSpace(t); t != "" {
				triggers = append(triggers, t)
			}
		}
	}
	if len(triggers) == 0 {
		// Auto-derive triggers from name words
		words := strings.Fields(strings.ToLower(name))
		triggers = words
	}

	meta := map[string]any{
		"id":               skillID,
		"name":             name,
		"description":      description,
		"tags":             tags,
		"trigger_patterns": triggers,
		"status":           skillActive,
		"version":          1,
		"created_at":       host.NowMs(),
		"last_used_at":     host.NowMs(),
		"usage_count":      0,
		"session_id":       sessionID,
	}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("skill_meta:"+skillID, string(metaJSON))

	// Store procedure body in BlobStorage
	storedBlobID := host.BlobUpload("skill_procedure:"+skillID, procedure, "text/plain")
	if plexspaces.IsHostError(storedBlobID) {
		host.Warn(fmt.Sprintf("SkillStoreActor: blob upload failed for skill %s: %s", skillID, storedBlobID))
		// Fallback: store procedure in KV
		host.KVPut("skill_proc:"+skillID, procedure)
	} else {
		host.KVPut("skill_blob:"+skillID, storedBlobID)
	}

	// Index by tags in TupleSpace
	for _, tag := range tags {
		_ = host.TS().Write([]any{"skill_tag", tag, skillID, name})
	}
	// Index by trigger patterns
	for _, trigger := range triggers {
		_ = host.TS().Write([]any{"skill_trigger", trigger, skillID, name})
	}

	// Track in skills list
	existing := host.KVGet("skill_ids")
	ids := []string{}
	if existing != "" {
		ids = strings.Split(existing, ",")
	}
	ids = append(ids, skillID)
	host.KVPut("skill_ids", strings.Join(ids, ","))

	s.SkillCount++
	s.LearnCount++
	s.IncrCounter(host, "skills_learned")
	fireAudit("skill_proposed", fmt.Sprintf("skill_id=%s name=%s", skillID, name))
	host.Info(fmt.Sprintf("SkillStoreActor: proposed skill=%s id=%s", name, skillID))
	return marshal(map[string]any{"status": "ok", "skill_id": skillID, "name": name})
}

func (s *SkillStoreActor) matchSkills(p map[string]any) string {
	query := stringVal(p, "query", "")
	limit := intVal(p, "limit", 3)
	queryLower := strings.ToLower(query)

	found := map[string]map[string]any{}

	// Search by trigger patterns in TupleSpace
	allTriggers := host.TS().ReadAll([]any{"skill_trigger", nil, nil, nil})
	for _, t := range allTriggers {
		if len(t) < 4 {
			continue
		}
		trigger, _ := t[1].(string)
		skillID, _ := t[2].(string)
		if strings.Contains(queryLower, strings.ToLower(trigger)) {
			if _, seen := found[skillID]; !seen {
				if meta := s.loadSkillMeta(skillID); meta != nil {
					if stringVal(meta, "status", "") == skillActive {
						found[skillID] = meta
					}
				}
			}
		}
	}

	// Also search tag index
	allTags := host.TS().ReadAll([]any{"skill_tag", nil, nil, nil})
	for _, t := range allTags {
		if len(t) < 4 {
			continue
		}
		tag, _ := t[1].(string)
		skillID, _ := t[2].(string)
		if strings.Contains(queryLower, strings.ToLower(tag)) {
			if _, seen := found[skillID]; !seen {
				if meta := s.loadSkillMeta(skillID); meta != nil {
					if stringVal(meta, "status", "") == skillActive {
						found[skillID] = meta
					}
				}
			}
		}
	}

	skills := make([]any, 0, len(found))
	for skillID, meta := range found {
		// Load procedure
		procedure := s.loadProcedure(skillID)
		enriched := map[string]any{}
		for k, v := range meta {
			enriched[k] = v
		}
		enriched["procedure"] = procedure
		skills = append(skills, enriched)
		if len(skills) >= limit {
			break
		}
	}

	s.MatchCount++
	return marshal(map[string]any{"status": "ok", "skills": skills, "count": len(skills), "query": query})
}

func (s *SkillStoreActor) listSkills(p map[string]any) string {
	status := stringVal(p, "status", "")
	existing := host.KVGet("skill_ids")
	if existing == "" {
		return marshal(map[string]any{"status": "ok", "skills": []any{}, "count": 0})
	}
	ids := strings.Split(existing, ",")
	skills := make([]any, 0, len(ids))
	for _, id := range ids {
		if id == "" {
			continue
		}
		meta := s.loadSkillMeta(id)
		if meta == nil {
			continue
		}
		if status != "" && stringVal(meta, "status", "") != status {
			continue
		}
		skills = append(skills, meta)
	}
	return marshal(map[string]any{"status": "ok", "skills": skills, "count": len(skills)})
}

func (s *SkillStoreActor) getSkill(p map[string]any) string {
	skillID := stringVal(p, "skill_id", "")
	if skillID == "" {
		return marshal(map[string]any{"error": "skill_id is required"})
	}
	meta := s.loadSkillMeta(skillID)
	if meta == nil {
		return marshal(map[string]any{"error": "skill not found", "skill_id": skillID})
	}
	meta["procedure"] = s.loadProcedure(skillID)
	meta["status_val"] = "ok"
	return marshal(meta)
}

func (s *SkillStoreActor) updateSkill(p map[string]any) string {
	skillID := stringVal(p, "skill_id", "")
	if skillID == "" {
		return marshal(map[string]any{"error": "skill_id is required"})
	}
	meta := s.loadSkillMeta(skillID)
	if meta == nil {
		return marshal(map[string]any{"error": "skill not found"})
	}
	if name := stringVal(p, "name", ""); name != "" {
		meta["name"] = name
	}
	if desc := stringVal(p, "description", ""); desc != "" {
		meta["description"] = desc
	}
	if proc := stringVal(p, "procedure", ""); proc != "" {
		updatedBlobID := host.BlobUpload("skill_procedure:"+skillID, proc, "text/plain")
		if plexspaces.IsHostError(updatedBlobID) {
			host.KVPut("skill_proc:"+skillID, proc)
		} else {
			host.KVPut("skill_blob:"+skillID, updatedBlobID)
		}
	}
	if v, ok := meta["version"].(float64); ok {
		meta["version"] = int(v) + 1
	}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("skill_meta:"+skillID, string(metaJSON))
	return marshal(map[string]any{"status": "ok", "skill_id": skillID})
}

func (s *SkillStoreActor) deleteSkill(p map[string]any) string {
	skillID := stringVal(p, "skill_id", "")
	if skillID == "" {
		return marshal(map[string]any{"error": "skill_id is required"})
	}
	host.KVDelete("skill_meta:" + skillID)
	host.KVDelete("skill_proc:" + skillID)
	host.KVDelete("skill_blob:" + skillID)

	existing := host.KVGet("skill_ids")
	if existing != "" {
		ids := strings.Split(existing, ",")
		newIDs := make([]string, 0, len(ids))
		for _, id := range ids {
			if id != skillID {
				newIDs = append(newIDs, id)
			}
		}
		host.KVPut("skill_ids", strings.Join(newIDs, ","))
	}
	if s.SkillCount > 0 {
		s.SkillCount--
	}
	return marshal(map[string]any{"status": "ok", "skill_id": skillID})
}

// evaluateForLearning inspects a conversation for learning opportunities.
// Fires a skill proposal when multi-step tool usage is detected.
func (s *SkillStoreActor) evaluateForLearning(p map[string]any) string {
	toolCallCount := intVal(p, "tool_call_count", 0)
	sessionID := stringVal(p, "session_id", "")

	if toolCallCount < 3 {
		return marshal(map[string]any{"status": "ok", "action": "no_learning", "reason": "too_few_tool_calls"})
	}

	// Parse messages to extract tool call sequence
	msgsRaw := stringVal(p, "messages", "")
	var messages []map[string]any
	if msgsRaw != "" {
		_ = json.Unmarshal([]byte(msgsRaw), &messages)
	}

	// Extract tool sequence
	toolSeq := []string{}
	userIntent := ""
	for _, m := range messages {
		if stringVal(m, "role", "") == "user" {
			userIntent = stringVal(m, "content", "")
		}
		if tcs, ok := m["tool_calls"].([]any); ok {
			for _, tc := range tcs {
				if tcm, ok := tc.(map[string]any); ok {
					toolSeq = append(toolSeq, stringVal(tcm, "name", ""))
				}
			}
		}
	}

	if len(toolSeq) < 3 || userIntent == "" {
		return marshal(map[string]any{"status": "ok", "action": "no_learning"})
	}

	// Check if this pattern was already learned
	patternKey := strings.Join(toolSeq, "→")
	existing := host.KVGet("learned_pattern:" + llmCacheKeyFor(patternKey))
	if existing != "" {
		return marshal(map[string]any{"status": "ok", "action": "pattern_already_known"})
	}
	host.KVPut("learned_pattern:"+llmCacheKeyFor(patternKey), sessionID)

	// Generate a skill from the pattern
	skillName := fmt.Sprintf("Auto: %s", truncateStr(userIntent, 40))
	procedure := fmt.Sprintf("When asked to: %s\nUse this tool sequence:\n", userIntent)
	for i, t := range toolSeq {
		procedure += fmt.Sprintf("%d. Call %s\n", i+1, t)
	}
	tags := strings.Join(uniqueWords(strings.ToLower(userIntent)), ",")

	return s.proposeSkill(map[string]any{
		"name":             skillName,
		"description":      fmt.Sprintf("Auto-learned from session %s: %d tool calls", sessionID, toolCallCount),
		"procedure":        procedure,
		"tags":             tags,
		"trigger_patterns": strings.Join(strings.Fields(strings.ToLower(userIntent))[:min(5, len(strings.Fields(userIntent)))], ","),
		"session_id":       sessionID,
	})
}

func (s *SkillStoreActor) recordUsage(p map[string]any) string {
	skillID := stringVal(p, "skill_id", "")
	if skillID == "" {
		return marshal(map[string]any{"error": "skill_id required"})
	}
	meta := s.loadSkillMeta(skillID)
	if meta == nil {
		return marshal(map[string]any{"error": "skill not found"})
	}
	usage := intVal(meta, "usage_count", 0)
	meta["usage_count"] = usage + 1
	meta["last_used_at"] = host.NowMs()
	meta["status"] = skillActive // reactivate if stale
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("skill_meta:"+skillID, string(metaJSON))
	s.IncrCounter(host, "skill_usages")
	return marshal(map[string]any{"status": "ok", "skill_id": skillID, "usage_count": usage + 1})
}

// maintenanceTick transitions stale/archived skills based on last_used_at.
func (s *SkillStoreActor) maintenanceTick() string {
	now := host.NowMs()
	staleThresholdMs := uint64(30 * 24 * 3600 * 1000)  // 30 days
	archiveThresholdMs := uint64(90 * 24 * 3600 * 1000) // 90 days

	existing := host.KVGet("skill_ids")
	if existing == "" {
		_ = host.SendAfter(86400000, "maintenance_tick", map[string]any{"op": "maintenance_tick"})
		return marshal(map[string]any{"status": "ok", "checked": 0})
	}

	ids := strings.Split(existing, ",")
	staled, archived := 0, 0
	for _, id := range ids {
		if id == "" {
			continue
		}
		meta := s.loadSkillMeta(id)
		if meta == nil {
			continue
		}
		lastUsed, _ := meta["last_used_at"].(float64)
		age := now - uint64(lastUsed)
		currentStatus := stringVal(meta, "status", skillActive)

		if age > archiveThresholdMs && currentStatus != skillArchived {
			meta["status"] = skillArchived
			metaJSON, _ := json.Marshal(meta)
			host.KVPut("skill_meta:"+id, string(metaJSON))
			archived++
			s.ArchiveCount++
		} else if age > staleThresholdMs && currentStatus == skillActive {
			meta["status"] = skillStale
			metaJSON, _ := json.Marshal(meta)
			host.KVPut("skill_meta:"+id, string(metaJSON))
			staled++
		}
	}

	_ = host.SendAfter(86400000, "maintenance_tick", map[string]any{"op": "maintenance_tick"})
	host.Info(fmt.Sprintf("SkillStoreActor: maintenance done staled=%d archived=%d", staled, archived))
	fireAudit("skill_maintenance", fmt.Sprintf("staled=%d archived=%d", staled, archived))
	return marshal(map[string]any{"status": "ok", "checked": len(ids), "staled": staled, "archived": archived})
}

func (s *SkillStoreActor) loadSkillMeta(skillID string) map[string]any {
	raw := host.KVGet("skill_meta:" + skillID)
	if raw == "" {
		return nil
	}
	var meta map[string]any
	if err := json.Unmarshal([]byte(raw), &meta); err != nil {
		return nil
	}
	return meta
}

func (s *SkillStoreActor) loadProcedure(skillID string) string {
	// Try BlobStorage first
	blobID := host.KVGet("skill_blob:" + skillID)
	if blobID != "" {
		data := host.BlobDownload(blobID)
		if data != "" && !plexspaces.IsHostError(data) {
			return data
		}
	}
	// Fallback to KV
	return host.KVGet("skill_proc:" + skillID)
}

func truncateStr(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen] + "..."
}

func uniqueWords(s string) []string {
	seen := map[string]bool{}
	result := []string{}
	for _, w := range strings.Fields(s) {
		w = strings.Trim(w, ".,!?;:")
		if len(w) > 3 && !seen[w] {
			seen[w] = true
			result = append(result, w)
		}
	}
	return result
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
