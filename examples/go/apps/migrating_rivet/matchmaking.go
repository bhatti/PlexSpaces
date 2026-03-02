// Game Matchmaking Service - Virtual Actor per Game Room (Go WASM)
//
// Demonstrates Rivet-style virtual actors for game matchmaking:
// - GameRoom actor per game room with virtual actor pattern
// - Auto-activation when first player joins
// - Auto-deactivation after idle timeout (5 minutes)
// - Matchmaking logic: match players by skill level and region
// - Timer-based cleanup for abandoned rooms
//
// Real-world use case: Multiplayer game matchmaking (like Rivet, Discord game rooms).
//
// ## SDK Features Used
//
// - plexspaces.BaseActor: Go actor base with JSON state serialization
// - plexspaces.Host: Host function wrappers (Ask, NowMs, Info, etc.)
// - Init(): Actor initialization from framework config
// - Handle(): Message routing by msg-type
// - GetState()/SetState(): Checkpoint-based state persistence
// - Virtual Actor Pattern: Auto-activation/deactivation via VirtualActorFacet
//
// ## Comparison to Rivet
//
// | Rivet                          | PlexSpaces Go                     |
// |--------------------------------|-----------------------------------|
// | rivet.actors.getOrActivate()   | VirtualActorFacet (automatic)     |
// | await room.join(player)        | host.Ask(roomID, "join", player)  |
// | await room.matchmake()         | host.Ask(roomID, "matchmake")     |
// | Automatic deactivation         | VirtualActorFacet idle timeout    |
// | State persistence              | DurabilityFacet                   |

package main

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

// ========================================================================
// Game Room Actor
// ========================================================================

// GameRoom implements a virtual actor for game matchmaking.
// Each room is a virtual actor that auto-activates on first player join
// and auto-deactivates after idle timeout.
type GameRoom struct {
	plexspaces.BaseActor

	// Configuration
	RoomID        string `json:"room_id"`
	MaxPlayers    int    `json:"max_players"`
	MinPlayers   int    `json:"min_players"`
	GameMode      string `json:"game_mode"`      // "2v2", "5v5", "battle_royale"
	IdleTimeoutMs uint64 `json:"idle_timeout_ms"` // Auto-deactivate after idle

	// Room state
	Players      []Player `json:"players"`
	Status       string   `json:"status"` // "waiting", "matching", "matched", "in_game"
	CreatedAtMs  uint64   `json:"created_at_ms"`
	LastActiveMs uint64   `json:"last_active_ms"`

	// Matchmaking state
	MatchmakingQueue []Player `json:"matchmaking_queue"`
	MatchedTeams     [][]Player `json:"matched_teams"`

	// Benchmark tracking
	TotalJoins       int     `json:"total_joins"`
	TotalMatches     int     `json:"total_matches"`
	TotalDeactivations int   `json:"total_deactivations"`
	TotalComputeMs   float64 `json:"total_compute_ms"`
	TotalCoordMs     float64 `json:"total_coord_ms"`
}

// Player represents a player in the matchmaking system
type Player struct {
	PlayerID   string  `json:"player_id"`
	SkillLevel int     `json:"skill_level"` // 1-100
	Region     string  `json:"region"`      // "us-east", "eu-west", etc.
	JoinedAtMs uint64  `json:"joined_at_ms"`
}

var host = plexspaces.NewHost()

func NewGameRoom() *GameRoom {
	a := &GameRoom{
		MaxPlayers:    10,
		MinPlayers:   2,
		GameMode:      "5v5",
		IdleTimeoutMs: 300000, // 5 minutes default
		Players:       make([]Player, 0),
		Status:        "waiting",
		MatchmakingQueue: make([]Player, 0),
		MatchedTeams:  make([][]Player, 0),
	}
	a.SetSelf(a)
	return a
}

func (g *GameRoom) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	g.RoomID = config.ActorID

	nowMs := host.NowMs()
	g.CreatedAtMs = nowMs
	g.LastActiveMs = nowMs

	if args := config.Args; args != nil {
		if v, ok := args["max_players"]; ok {
			g.MaxPlayers = toInt(v)
		}
		if v, ok := args["min_players"]; ok {
			g.MinPlayers = toInt(v)
		}
		if v, ok := args["game_mode"]; ok {
			g.GameMode = toString(v)
		}
		if v, ok := args["idle_timeout_ms"]; ok {
			g.IdleTimeoutMs = toUint64(v)
		}
	}

	// Return empty string for success (non-empty = error)
	return ""
}

func (g *GameRoom) Handle(from string, msgType string, payload string) string {
	startMs := host.NowMs()

	var result string
	switch msgType {
	case "join":
		result = g.handleJoin(payload)
	case "leave":
		result = g.handleLeave(payload)
	case "matchmake":
		result = g.handleMatchmake()
	case "get_status":
		result = g.handleGetStatus()
	case "get_stats":
		result = g.handleGetStats()
	default:
		result = `{"status":"error","error":"unknown operation"}`
	}

	computeMs := float64(host.NowMs() - startMs)
	g.TotalComputeMs += computeMs
	g.LastActiveMs = host.NowMs()

	return result
}

func (g *GameRoom) handleJoin(payload string) string {
	var req struct {
		PlayerID   string `json:"player_id"`
		SkillLevel int    `json:"skill_level"`
		Region     string `json:"region"`
	}
	if err := json.Unmarshal([]byte(payload), &req); err != nil {
		return fmt.Sprintf(`{"status":"error","error":"invalid payload: %s"}`, err.Error())
	}

	// Check if player already in room
	for _, p := range g.Players {
		if p.PlayerID == req.PlayerID {
			return `{"status":"ok","message":"player already in room"}`
		}
	}

	// Check if room is full
	if len(g.Players) >= g.MaxPlayers {
		return `{"status":"error","error":"room is full"}`
	}

	// Add player
	player := Player{
		PlayerID:   req.PlayerID,
		SkillLevel: req.SkillLevel,
		Region:     req.Region,
		JoinedAtMs: host.NowMs(),
	}
	g.Players = append(g.Players, player)
	g.TotalJoins++
	g.Status = "waiting"

	return fmt.Sprintf(`{"status":"ok","room_id":"%s","players_count":%d,"max_players":%d}`,
		g.RoomID, len(g.Players), g.MaxPlayers)
}

func (g *GameRoom) handleLeave(payload string) string {
	var req struct {
		PlayerID string `json:"player_id"`
	}
	if err := json.Unmarshal([]byte(payload), &req); err != nil {
		return fmt.Sprintf(`{"status":"error","error":"invalid payload: %s"}`, err.Error())
	}

	// Remove player
	for i, p := range g.Players {
		if p.PlayerID == req.PlayerID {
			g.Players = append(g.Players[:i], g.Players[i+1:]...)
			break
		}
	}

	// Update status
	if len(g.Players) == 0 {
		g.Status = "waiting"
	}

	return fmt.Sprintf(`{"status":"ok","players_count":%d}`, len(g.Players))
}

func (g *GameRoom) handleMatchmake() string {
	if len(g.Players) < g.MinPlayers {
		return fmt.Sprintf(`{"status":"error","error":"not enough players (need %d, have %d)"}`,
			g.MinPlayers, len(g.Players))
	}

	// Matchmaking algorithm: group players by skill level and region
	matched := g.matchPlayers()
	if len(matched) == 0 {
		return `{"status":"ok","matched":false,"message":"no suitable matches found"}`
	}

	g.MatchedTeams = matched
	g.Status = "matched"
	g.TotalMatches++

	// Format response
	teamsJSON, _ := json.Marshal(matched)
	return fmt.Sprintf(`{"status":"ok","matched":true,"teams":%s}`, string(teamsJSON))
}

func (g *GameRoom) matchPlayers() [][]Player {
	// Simple matchmaking: group players by skill level (within ±10) and same region
	players := make([]Player, len(g.Players))
	copy(players, g.Players)

	// Sort by skill level
	sort.Slice(players, func(i, j int) bool {
		return players[i].SkillLevel < players[j].SkillLevel
	})

	teams := make([][]Player, 0)
	used := make(map[int]bool)

	for i := 0; i < len(players); i++ {
		if used[i] {
			continue
		}

		team := []Player{players[i]}
		used[i] = true

		// Find compatible players (same region, skill within ±10)
		for j := i + 1; j < len(players) && len(team) < g.MinPlayers; j++ {
			if used[j] {
				continue
			}

			if players[j].Region == players[i].Region &&
				math.Abs(float64(players[j].SkillLevel-players[i].SkillLevel)) <= 10 {
				team = append(team, players[j])
				used[j] = true
			}
		}

		if len(team) >= g.MinPlayers {
			teams = append(teams, team)
		}
	}

	return teams
}

func (g *GameRoom) handleGetStatus() string {
	statusJSON, _ := json.Marshal(map[string]any{
		"status":        g.Status,
		"room_id":       g.RoomID,
		"players_count": len(g.Players),
		"max_players":   g.MaxPlayers,
		"min_players":   g.MinPlayers,
		"game_mode":     g.GameMode,
		"players":       g.Players,
		"matched_teams": g.MatchedTeams,
	})
	return fmt.Sprintf(`{"status":"ok","payload":%s}`, string(statusJSON))
}

func (g *GameRoom) handleGetStats() string {
	// Calculate benchmarks
	totalMs := g.TotalComputeMs + g.TotalCoordMs
	computePct := 0.0
	coordPct := 0.0
	if totalMs > 0 {
		computePct = (g.TotalComputeMs / totalMs) * 100
		coordPct = (g.TotalCoordMs / totalMs) * 100
	}
	granularity := 0.0
	if g.TotalCoordMs > 0 {
		granularity = g.TotalComputeMs / g.TotalCoordMs
	}

	statsJSON, _ := json.Marshal(map[string]any{
		"config": map[string]any{
			"room_id":         g.RoomID,
			"max_players":     g.MaxPlayers,
			"min_players":     g.MinPlayers,
			"game_mode":       g.GameMode,
			"idle_timeout_ms": g.IdleTimeoutMs,
		},
		"counters": map[string]any{
			"total_joins":         g.TotalJoins,
			"total_matches":       g.TotalMatches,
			"total_deactivations": g.TotalDeactivations,
			"current_players":     len(g.Players),
		},
		"benchmarks": map[string]any{
			"total_ms":      totalMs,
			"compute_ms":    g.TotalComputeMs,
			"coord_ms":      g.TotalCoordMs,
			"compute_pct":   computePct,
			"coord_pct":     coordPct,
			"granularity":   granularity,
		},
	})
	return fmt.Sprintf(`{"status":"ok","payload":%s}`, string(statsJSON))
}

// ========================================================================
// Type Conversion Helpers (local to this example)
// ========================================================================
//
// These helpers simplify extracting typed values from map[string]any
// (common when parsing JSON config args). They handle common type
// conversions safely.

// toInt converts a value to int, handling common types.
// Returns 0 if conversion fails.
func toInt(v any) int {
	switch val := v.(type) {
	case int:
		return val
	case int8:
		return int(val)
	case int16:
		return int(val)
	case int32:
		return int(val)
	case int64:
		return int(val)
	case uint:
		return int(val)
	case uint8:
		return int(val)
	case uint16:
		return int(val)
	case uint32:
		return int(val)
	case uint64:
		return int(val)
	case float32:
		return int(val)
	case float64:
		return int(val)
	case string:
		var i int
		if _, err := fmt.Sscanf(val, "%d", &i); err == nil {
			return i
		}
		return 0
	default:
		return 0
	}
}

// toUint64 converts a value to uint64, handling common types.
// Returns 0 if conversion fails.
func toUint64(v any) uint64 {
	switch val := v.(type) {
	case int:
		return uint64(val)
	case int8:
		return uint64(val)
	case int16:
		return uint64(val)
	case int32:
		return uint64(val)
	case int64:
		return uint64(val)
	case uint:
		return uint64(val)
	case uint8:
		return uint64(val)
	case uint16:
		return uint64(val)
	case uint32:
		return uint64(val)
	case uint64:
		return val
	case float32:
		return uint64(val)
	case float64:
		return uint64(val)
	case string:
		var i uint64
		if _, err := fmt.Sscanf(val, "%d", &i); err == nil {
			return i
		}
		return 0
	default:
		return 0
	}
}

// toString converts a value to string, handling common types.
func toString(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	return fmt.Sprintf("%v", v)
}

func init() {
	// Register the actor for WASM export
	// CRITICAL: Registration must be in init(), not main()
	// WASM reactor adapter calls _initialize (runs init()) but NOT _start (runs main())
	plexspaces.Register(NewGameRoom())
}

func main() {}
