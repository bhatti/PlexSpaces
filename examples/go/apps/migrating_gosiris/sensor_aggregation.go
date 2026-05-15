// IoT Sensor Aggregation - Temperature/Humidity Monitoring (Go WASM)
//
// Demonstrates Gosiris-style actor model for IoT sensor data collection:
// - SensorActor generates simulated temperature/humidity readings
// - AggregatorActor collects and computes rolling statistics across sensors
// - Process groups for dynamic sensor discovery and broadcast
//
// Real-world use case: Industrial IoT monitoring (factory floors, data centers,
// agriculture) where thousands of sensors report to aggregation actors that
// compute rolling statistics, detect anomalies, and trigger alerts.
//
// ## SDK Features Used
//
// - plexspaces.BaseActor: Go actor with JSON state serialization
// - plexspaces.ActorRouter: Multi-actor routing in single WASM module
// - plexspaces.Host.PG(): Process group join/members/broadcast
// - plexspaces.Host.Ask(): Request-reply for sensor polling
// - plexspaces.Host.NowMs(): Timestamped readings
//
// ## Comparison to Gosiris
//
// | Gosiris (Native Go)              | PlexSpaces Go WASM                    |
// |----------------------------------|---------------------------------------|
// | gosiris.Actor interface           | plexspaces.Actor interface            |
// | actor.Receive(ctx, msg)          | actor.Handle(from, msgType, payload)  |
// | system.ActorOf("name", &Actor{}) | app-config.toml supervisor children   |
// | ctx.Send(ref, msg)               | host.Send(actorID, msgType, payload)  |
// | Manual actor registry            | ActorRouter prefix-based routing      |
// | In-memory only                   | GetState/SetState + KV persistence    |
// | Single process                   | Distributed across WASM nodes         |

package main

import (
	"encoding/json"
	"fmt"
	"math"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ========================================================================
// Sensor Actor - Simulates IoT sensor readings
// ========================================================================

// SensorActor simulates an IoT sensor that produces temperature and humidity
// readings. Each sensor maintains a reading history and joins a process group
// for discovery by aggregators.
type SensorActor struct {
	plexspaces.BaseActor

	SensorID   string           `json:"sensor_id"`
	Location   string           `json:"location"`
	SensorType string           `json:"sensor_type"`
	Readings   []SensorReading  `json:"readings"`
	MaxHistory int              `json:"max_history"`
	ReadCount  int              `json:"read_count"`

	// Simulation state (deterministic pseudo-random via linear congruential)
	BaseTempC    float64 `json:"base_temp_c"`
	BaseHumidity float64 `json:"base_humidity"`
	DriftRate    float64 `json:"drift_rate"`
	NoiseScale   float64 `json:"noise_scale"`
	Seed         uint64  `json:"seed"`
}

type SensorReading struct {
	SensorID    string  `json:"sensor_id"`
	TempC       float64 `json:"temp_c"`
	Humidity    float64 `json:"humidity"`
	TimestampMs uint64  `json:"timestamp_ms"`
	SeqNum      int     `json:"seq_num"`
}

func NewSensorActor() plexspaces.Actor {
	a := &SensorActor{
		MaxHistory:   500,
		BaseTempC:    22.0,
		BaseHumidity: 45.0,
		DriftRate:    0.01,
		NoiseScale:   0.5,
		Seed:         12345,
	}
	a.SetSelf(a)
	return a
}

func (s *SensorActor) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SensorID = config.ActorID
	s.Readings = make([]SensorReading, 0)

	if args := config.Args; args != nil {
		if v, ok := args["location"]; ok {
			s.Location = fmt.Sprintf("%v", v)
		}
		if v, ok := args["sensor_type"]; ok {
			s.SensorType = fmt.Sprintf("%v", v)
		}
		if v, ok := args["base_temp_c"]; ok {
			s.BaseTempC = toFloat64(v)
		}
		if v, ok := args["base_humidity"]; ok {
			s.BaseHumidity = toFloat64(v)
		}
		if v, ok := args["max_history"]; ok {
			s.MaxHistory = toInt(v)
		}
	}
	if s.Location == "" {
		s.Location = "zone-default"
	}
	if s.SensorType == "" {
		s.SensorType = "temp_humidity"
	}
	if s.MaxHistory == 0 {
		s.MaxHistory = 500
	}

	// Use sensor ID hash as seed for deterministic but varied readings per sensor
	s.Seed = hashString(s.SensorID)

	// Join the sensors process group for discovery
	if err := host.PG().Join("sensors"); err != nil {
		host.Warn(fmt.Sprintf("Sensor %s: failed to join process group: %v", s.SensorID, err))
	}

	// Also join location-specific group
	if err := host.PG().Join("sensors:" + s.Location); err != nil {
		host.Warn(fmt.Sprintf("Sensor %s: failed to join location group: %v", s.SensorID, err))
	}

	host.Info(fmt.Sprintf("Sensor %s initialized: location=%s type=%s base_temp=%.1fC base_humidity=%.1f%%",
		s.SensorID, s.Location, s.SensorType, s.BaseTempC, s.BaseHumidity))
	return ""
}

func (s *SensorActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "read":
		return s.readSensor()
	case "read_batch":
		return s.readBatch(payloadJSON)
	case "get_latest":
		return s.getLatest()
	case "get_history":
		return s.getHistory(payloadJSON)
	case "stats":
		return s.getStats()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// readSensor generates a new simulated reading with realistic noise/drift.
func (s *SensorActor) readSensor() string {
	now := host.NowMs()
	s.ReadCount++

	// Generate deterministic pseudo-random noise
	s.Seed = lcgNext(s.Seed)
	tempNoise := (lcgFloat(s.Seed) - 0.5) * 2 * s.NoiseScale
	s.Seed = lcgNext(s.Seed)
	humNoise := (lcgFloat(s.Seed) - 0.5) * 2 * s.NoiseScale * 2

	// Apply slow drift over time (simulates environmental changes)
	drift := math.Sin(float64(s.ReadCount)*s.DriftRate) * 2.0

	reading := SensorReading{
		SensorID:    s.SensorID,
		TempC:       math.Round((s.BaseTempC+drift+tempNoise)*100) / 100,
		Humidity:    math.Round(clamp(s.BaseHumidity+drift*1.5+humNoise, 0, 100)*100) / 100,
		TimestampMs: now,
		SeqNum:      s.ReadCount,
	}

	s.Readings = append(s.Readings, reading)

	// Trim history if needed
	if len(s.Readings) > s.MaxHistory {
		s.Readings = s.Readings[len(s.Readings)-s.MaxHistory:]
	}

	return marshal(map[string]any{
		"status":  "ok",
		"reading": reading,
	})
}

// readBatch generates multiple readings at once for benchmarking.
func (s *SensorActor) readBatch(payloadJSON string) string {
	var req struct {
		Count int `json:"count"`
	}
	json.Unmarshal([]byte(payloadJSON), &req)
	if req.Count <= 0 {
		req.Count = 100
	}

	startMs := host.NowMs()
	readings := make([]SensorReading, 0, req.Count)

	for i := 0; i < req.Count; i++ {
		s.ReadCount++
		s.Seed = lcgNext(s.Seed)
		tempNoise := (lcgFloat(s.Seed) - 0.5) * 2 * s.NoiseScale
		s.Seed = lcgNext(s.Seed)
		humNoise := (lcgFloat(s.Seed) - 0.5) * 2 * s.NoiseScale * 2
		drift := math.Sin(float64(s.ReadCount)*s.DriftRate) * 2.0

		reading := SensorReading{
			SensorID:    s.SensorID,
			TempC:       math.Round((s.BaseTempC+drift+tempNoise)*100) / 100,
			Humidity:    math.Round(clamp(s.BaseHumidity+drift*1.5+humNoise, 0, 100)*100) / 100,
			TimestampMs: startMs,
			SeqNum:      s.ReadCount,
		}
		readings = append(readings, reading)
	}

	// Keep only last max_history
	s.Readings = append(s.Readings, readings...)
	if len(s.Readings) > s.MaxHistory {
		s.Readings = s.Readings[len(s.Readings)-s.MaxHistory:]
	}

	endMs := host.NowMs()
	durationMs := float64(endMs - startMs)
	opsPerSec := 0.0
	if durationMs > 0 {
		opsPerSec = float64(req.Count) / (durationMs / 1000.0)
	}

	return marshal(map[string]any{
		"status":      "ok",
		"count":       req.Count,
		"duration_ms": durationMs,
		"ops_per_sec": opsPerSec,
		"history_size": len(s.Readings),
	})
}

func (s *SensorActor) getLatest() string {
	if len(s.Readings) == 0 {
		return marshal(map[string]any{"status": "ok", "reading": nil})
	}
	return marshal(map[string]any{
		"status":  "ok",
		"reading": s.Readings[len(s.Readings)-1],
	})
}

func (s *SensorActor) getHistory(payloadJSON string) string {
	var req struct {
		Limit int `json:"limit"`
	}
	json.Unmarshal([]byte(payloadJSON), &req)
	if req.Limit <= 0 || req.Limit > len(s.Readings) {
		req.Limit = len(s.Readings)
	}
	start := len(s.Readings) - req.Limit
	return marshal(map[string]any{
		"status":   "ok",
		"readings": s.Readings[start:],
		"count":    req.Limit,
		"total":    len(s.Readings),
	})
}

func (s *SensorActor) getStats() string {
	stats := computeStats(s.Readings)
	return marshal(map[string]any{
		"status":      "ok",
		"sensor_id":   s.SensorID,
		"location":    s.Location,
		"sensor_type": s.SensorType,
		"read_count":  s.ReadCount,
		"history_size": len(s.Readings),
		"stats":       stats,
	})
}

// ========================================================================
// Aggregator Actor - Collects and analyzes sensor data
// ========================================================================

// AggregatorActor collects readings from sensor actors via process groups,
// computes rolling statistics, and detects anomalies across the sensor network.
type AggregatorActor struct {
	plexspaces.BaseActor

	AggregatorID  string                    `json:"aggregator_id"`
	SensorData    map[string]*SensorSummary `json:"sensor_data"`
	MaxReadings   int                       `json:"max_readings"`
	AlertThreshTempC  float64              `json:"alert_thresh_temp_c"`
	AlertThreshHumid  float64              `json:"alert_thresh_humid"`

	// Counters
	TotalPolls       int     `json:"total_polls"`
	TotalReadings    int     `json:"total_readings"`
	TotalAlerts      int     `json:"total_alerts"`
	TotalComputeMs   float64 `json:"total_compute_ms"`
	TotalCoordMs     float64 `json:"total_coord_ms"`
}

type SensorSummary struct {
	SensorID     string          `json:"sensor_id"`
	Location     string          `json:"location"`
	Readings     []SensorReading `json:"readings"`
	LastPollMs   uint64          `json:"last_poll_ms"`
	TempStats    Stats           `json:"temp_stats"`
	HumidStats   Stats           `json:"humid_stats"`
	Anomalies    int             `json:"anomalies"`
}

type Stats struct {
	Count  int     `json:"count"`
	Mean   float64 `json:"mean"`
	StdDev float64 `json:"std_dev"`
	Min    float64 `json:"min"`
	Max    float64 `json:"max"`
}

func NewAggregatorActor() plexspaces.Actor {
	a := &AggregatorActor{
		SensorData:       make(map[string]*SensorSummary),
		MaxReadings:      200,
		AlertThreshTempC: 5.0,
		AlertThreshHumid: 20.0,
	}
	a.SetSelf(a)
	return a
}

func (ag *AggregatorActor) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	ag.AggregatorID = config.ActorID

	if args := config.Args; args != nil {
		if v, ok := args["max_readings"]; ok {
			ag.MaxReadings = toInt(v)
		}
		if v, ok := args["alert_thresh_temp_c"]; ok {
			ag.AlertThreshTempC = toFloat64(v)
		}
		if v, ok := args["alert_thresh_humid"]; ok {
			ag.AlertThreshHumid = toFloat64(v)
		}
	}
	if ag.MaxReadings == 0 {
		ag.MaxReadings = 200
	}

	host.Info(fmt.Sprintf("Aggregator %s initialized: max_readings=%d alert_temp=%.1fC alert_humid=%.1f%%",
		ag.AggregatorID, ag.MaxReadings, ag.AlertThreshTempC, ag.AlertThreshHumid))
	return ""
}

func (ag *AggregatorActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "ingest":
		return ag.ingestReading(payloadJSON)
	case "ingest_batch":
		return ag.ingestBatch(payloadJSON)
	case "poll_sensors":
		return ag.pollSensors(payloadJSON)
	case "get_summary":
		return ag.getSummary(payloadJSON)
	case "get_alerts":
		return ag.getAlerts()
	case "get_network_stats":
		return ag.getNetworkStats()
	case "stats":
		return ag.getAggregatorStats()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// ingestReading adds a single reading from a sensor.
func (ag *AggregatorActor) ingestReading(payloadJSON string) string {
	var reading SensorReading
	if err := json.Unmarshal([]byte(payloadJSON), &reading); err != nil {
		return marshal(map[string]any{"error": "invalid reading: " + err.Error()})
	}

	computeStart := host.NowMs()
	ag.addReading(reading)
	computeEnd := host.NowMs()
	ag.TotalComputeMs += float64(computeEnd - computeStart)

	return marshal(map[string]any{
		"status":     "ok",
		"sensor_id":  reading.SensorID,
		"ingested":   1,
	})
}

// ingestBatch adds many readings at once for benchmarking.
func (ag *AggregatorActor) ingestBatch(payloadJSON string) string {
	var req struct {
		Readings []SensorReading `json:"readings"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid batch: " + err.Error()})
	}

	computeStart := host.NowMs()
	for _, r := range req.Readings {
		ag.addReading(r)
	}
	computeEnd := host.NowMs()
	computeMs := float64(computeEnd - computeStart)
	ag.TotalComputeMs += computeMs

	opsPerSec := 0.0
	if computeMs > 0 {
		opsPerSec = float64(len(req.Readings)) / (computeMs / 1000.0)
	}

	return marshal(map[string]any{
		"status":      "ok",
		"ingested":    len(req.Readings),
		"sensors":     len(ag.SensorData),
		"compute_ms":  math.Round(computeMs*100) / 100,
		"ops_per_sec": math.Round(opsPerSec*100) / 100,
	})
}

// pollSensors queries all sensors in the process group via host.Ask.
func (ag *AggregatorActor) pollSensors(payloadJSON string) string {
	var req struct {
		Group string `json:"group"`
	}
	json.Unmarshal([]byte(payloadJSON), &req)
	if req.Group == "" {
		req.Group = "sensors"
	}

	coordStart := host.NowMs()

	// Discover sensors via process group
	members, err := host.PG().Members(req.Group)
	if err != nil {
		return marshal(map[string]any{"error": "failed to get group members: " + err.Error()})
	}

	coordEnd := host.NowMs()
	ag.TotalCoordMs += float64(coordEnd - coordStart)

	polled := 0
	failed := 0
	computeTotal := float64(0)

	for _, sensorID := range members {
		coordStart = host.NowMs()
		resp, err := host.Ask(sensorID, "read", nil, 5000)
		coordEnd = host.NowMs()
		ag.TotalCoordMs += float64(coordEnd - coordStart)

		if err != nil {
			failed++
			continue
		}

		// Parse response and ingest the reading
		computeStart := host.NowMs()
		respMap, ok := resp.(map[string]any)
		if !ok {
			failed++
			continue
		}
		readingData, ok := respMap["reading"]
		if !ok {
			failed++
			continue
		}

		readingBytes, _ := json.Marshal(readingData)
		var reading SensorReading
		if err := json.Unmarshal(readingBytes, &reading); err != nil {
			failed++
			continue
		}

		ag.addReading(reading)
		polled++
		computeEnd := host.NowMs()
		computeTotal += float64(computeEnd - computeStart)
	}

	ag.TotalComputeMs += computeTotal
	ag.TotalPolls++

	return marshal(map[string]any{
		"status":       "ok",
		"sensors_found": len(members),
		"polled":        polled,
		"failed":        failed,
		"group":         req.Group,
	})
}

// addReading adds a reading and updates rolling statistics.
func (ag *AggregatorActor) addReading(reading SensorReading) {
	summary, exists := ag.SensorData[reading.SensorID]
	if !exists {
		summary = &SensorSummary{
			SensorID: reading.SensorID,
			Readings: make([]SensorReading, 0),
		}
		ag.SensorData[reading.SensorID] = summary
	}

	summary.Readings = append(summary.Readings, reading)
	summary.LastPollMs = reading.TimestampMs
	ag.TotalReadings++

	// Trim per-sensor history
	if len(summary.Readings) > ag.MaxReadings {
		summary.Readings = summary.Readings[len(summary.Readings)-ag.MaxReadings:]
	}

	// Recompute stats
	temps := make([]float64, len(summary.Readings))
	humids := make([]float64, len(summary.Readings))
	for i, r := range summary.Readings {
		temps[i] = r.TempC
		humids[i] = r.Humidity
	}
	summary.TempStats = calcStats(temps)
	summary.HumidStats = calcStats(humids)

	// Anomaly detection: reading outside mean +/- threshold
	if summary.TempStats.Count > 10 {
		tempDev := math.Abs(reading.TempC - summary.TempStats.Mean)
		humDev := math.Abs(reading.Humidity - summary.HumidStats.Mean)
		if tempDev > ag.AlertThreshTempC || humDev > ag.AlertThreshHumid {
			summary.Anomalies++
			ag.TotalAlerts++
		}
	}
}

func (ag *AggregatorActor) getSummary(payloadJSON string) string {
	var req struct {
		SensorID string `json:"sensor_id"`
	}
	json.Unmarshal([]byte(payloadJSON), &req)

	if req.SensorID != "" {
		summary, ok := ag.SensorData[req.SensorID]
		if !ok {
			return marshal(map[string]any{"status": "ok", "found": false})
		}
		return marshal(map[string]any{
			"status":      "ok",
			"found":       true,
			"sensor_id":   summary.SensorID,
			"readings":    len(summary.Readings),
			"temp_stats":  summary.TempStats,
			"humid_stats": summary.HumidStats,
			"anomalies":   summary.Anomalies,
		})
	}

	// Return all sensor summaries (without raw readings)
	summaries := make([]map[string]any, 0, len(ag.SensorData))
	for _, s := range ag.SensorData {
		summaries = append(summaries, map[string]any{
			"sensor_id":   s.SensorID,
			"readings":    len(s.Readings),
			"temp_stats":  s.TempStats,
			"humid_stats": s.HumidStats,
			"anomalies":   s.Anomalies,
		})
	}

	return marshal(map[string]any{
		"status":  "ok",
		"sensors": len(ag.SensorData),
		"data":    summaries,
	})
}

func (ag *AggregatorActor) getAlerts() string {
	alerts := make([]map[string]any, 0)
	for _, s := range ag.SensorData {
		if s.Anomalies > 0 {
			alerts = append(alerts, map[string]any{
				"sensor_id": s.SensorID,
				"anomalies": s.Anomalies,
				"temp_mean": s.TempStats.Mean,
				"temp_stddev": s.TempStats.StdDev,
				"humid_mean": s.HumidStats.Mean,
				"humid_stddev": s.HumidStats.StdDev,
			})
		}
	}
	return marshal(map[string]any{
		"status":       "ok",
		"total_alerts": ag.TotalAlerts,
		"sensors_with_anomalies": len(alerts),
		"alerts":       alerts,
	})
}

func (ag *AggregatorActor) getNetworkStats() string {
	totalReadings := 0
	totalAnomalies := 0
	allTemps := make([]float64, 0)
	allHumids := make([]float64, 0)

	for _, s := range ag.SensorData {
		totalReadings += len(s.Readings)
		totalAnomalies += s.Anomalies
		for _, r := range s.Readings {
			allTemps = append(allTemps, r.TempC)
			allHumids = append(allHumids, r.Humidity)
		}
	}

	return marshal(map[string]any{
		"status":          "ok",
		"total_sensors":   len(ag.SensorData),
		"total_readings":  totalReadings,
		"total_anomalies": totalAnomalies,
		"network_temp":    calcStats(allTemps),
		"network_humid":   calcStats(allHumids),
	})
}

func (ag *AggregatorActor) getAggregatorStats() string {
	totalTime := ag.TotalComputeMs + ag.TotalCoordMs
	computePct := 0.0
	coordPct := 0.0
	opsPerSec := 0.0
	granularity := 0.0

	if totalTime > 0 {
		computePct = ag.TotalComputeMs / totalTime * 100
		coordPct = ag.TotalCoordMs / totalTime * 100
		opsPerSec = float64(ag.TotalReadings) / (totalTime / 1000.0)
	}
	if ag.TotalCoordMs > 0 {
		granularity = ag.TotalComputeMs / ag.TotalCoordMs
	}

	totalReadingsStored := 0
	for _, s := range ag.SensorData {
		totalReadingsStored += len(s.Readings)
	}
	memoryKB := float64(totalReadingsStored*80+len(ag.SensorData)*256) / 1024.0

	return marshal(map[string]any{
		"status": "ok",
		"counters": map[string]any{
			"total_polls":    ag.TotalPolls,
			"total_readings": ag.TotalReadings,
			"total_alerts":   ag.TotalAlerts,
			"active_sensors": len(ag.SensorData),
		},
		"benchmarks": map[string]any{
			"total_ms":    math.Round(totalTime*100) / 100,
			"compute_ms":  math.Round(ag.TotalComputeMs*100) / 100,
			"coord_ms":    math.Round(ag.TotalCoordMs*100) / 100,
			"compute_pct": math.Round(computePct*100) / 100,
			"coord_pct":   math.Round(coordPct*100) / 100,
			"granularity": math.Round(granularity*100) / 100,
			"ops_per_sec": math.Round(opsPerSec*100) / 100,
			"memory_kb":   math.Round(memoryKB*100) / 100,
			"readings_stored": totalReadingsStored,
		},
	})
}

// ========================================================================
// Math/Statistics Helpers
// ========================================================================

func calcStats(values []float64) Stats {
	n := len(values)
	if n == 0 {
		return Stats{}
	}

	sum := 0.0
	minVal := values[0]
	maxVal := values[0]
	for _, v := range values {
		sum += v
		if v < minVal {
			minVal = v
		}
		if v > maxVal {
			maxVal = v
		}
	}
	mean := sum / float64(n)

	variance := 0.0
	for _, v := range values {
		diff := v - mean
		variance += diff * diff
	}
	if n > 1 {
		variance /= float64(n - 1)
	}

	return Stats{
		Count:  n,
		Mean:   math.Round(mean*100) / 100,
		StdDev: math.Round(math.Sqrt(variance)*100) / 100,
		Min:    math.Round(minVal*100) / 100,
		Max:    math.Round(maxVal*100) / 100,
	}
}

func computeStats(readings []SensorReading) map[string]any {
	if len(readings) == 0 {
		return map[string]any{"count": 0}
	}
	temps := make([]float64, len(readings))
	humids := make([]float64, len(readings))
	for i, r := range readings {
		temps[i] = r.TempC
		humids[i] = r.Humidity
	}
	return map[string]any{
		"temperature": calcStats(temps),
		"humidity":    calcStats(humids),
	}
}

// ========================================================================
// General Helpers
// ========================================================================

func marshal(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `{"error":"marshal failed"}`
	}
	return string(data)
}

func toFloat64(v any) float64 {
	switch n := v.(type) {
	case float64:
		return n
	case string:
		var result float64
		fmt.Sscanf(n, "%f", &result)
		return result
	default:
		return 0
	}
}

func toInt(v any) int {
	switch n := v.(type) {
	case float64:
		return int(n)
	case string:
		var result int
		fmt.Sscanf(n, "%d", &result)
		return result
	default:
		return 0
	}
}

func clamp(v, low, high float64) float64 {
	if v < low {
		return low
	}
	if v > high {
		return high
	}
	return v
}

// Linear congruential generator for deterministic pseudo-random numbers in WASM.
func lcgNext(seed uint64) uint64 {
	return seed*6364136223846793005 + 1442695040888963407
}

func lcgFloat(seed uint64) float64 {
	return float64(seed>>33) / float64(1<<31)
}

func hashString(s string) uint64 {
	h := uint64(5381)
	for _, c := range s {
		h = h*33 + uint64(c)
	}
	return h
}

// ========================================================================
// Registration - Register actors for WASM export
// ========================================================================

// init() runs during _initialize (before main), ensuring the actors are
// registered before the host calls any exported functions like init/handle.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("sensor", NewSensorActor)
	router.Route("aggregator", NewAggregatorActor)
	plexspaces.Register(router)
}

func main() {}
