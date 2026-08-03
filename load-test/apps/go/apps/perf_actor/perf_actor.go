// SPDX-License-Identifier: AGPL-3.0-or-later
// PerfActor — Go (TinyGo) WASM actor for PlexSpaces load testing.
//
// Operations: echo, compute (Mersenne prime), kv_put/kv_get, pg_broadcast, shard_task, get_stats.
// Identical semantics to the Python / TypeScript / Rust WASM variants.

package main

import (
	"encoding/json"
	"math/big"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// PerfActor holds all persistent state.
type PerfActor struct {
	plexspaces.BaseActor
	EchoCount    int `json:"echo_count"`
	ComputeCount int `json:"compute_count"`
	KvCount      int `json:"kv_count"`
	PgCount      int `json:"pg_count"`
	ShardCount   int `json:"shard_count"`
	ActorID      string `json:"actor_id"`
}

func newPerfActor() *PerfActor {
	a := &PerfActor{}
	a.SetSelf(a)
	return a
}

// isMersennePrime runs the Lucas-Lehmer test: returns true if 2^p - 1 is prime.
func isMersennePrime(p int) bool {
	if p == 2 {
		return true
	}
	if p < 2 {
		return false
	}
	// mp = 2^p - 1
	mp := new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), uint(p)), big.NewInt(1))
	s := big.NewInt(4)
	two := big.NewInt(2)
	for i := 0; i < p-2; i++ {
		s.Mod(s.Sub(s.Mul(s, s), two), mp)
	}
	return s.Sign() == 0
}

// gradientStep computes one gradient descent step on a float slice.
func gradientStep(values []float64, lr float64) map[string]interface{} {
	n := len(values)
	if n == 0 {
		return map[string]interface{}{"gradient": 0.0, "count": 0}
	}
	sum := 0.0
	for _, v := range values {
		sum += v
	}
	mean := sum / float64(n)
	grad := 0.0
	for _, v := range values {
		d := v - mean
		grad += d * d
	}
	grad /= float64(n)
	sample := values
	if len(sample) > 3 {
		sample = sample[:3]
	}
	return map[string]interface{}{
		"gradient": grad,
		"count":    n,
		"mean":     mean,
		"sample":   sample,
	}
}

func (a *PerfActor) Init(configJSON string) string {
	var cfg map[string]interface{}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	if id, ok := cfg["actor_id"].(string); ok {
		a.ActorID = id
	}
	return ""
}

func (a *PerfActor) Handle(fromActor, msgType, payloadJSON string) string {
	var payload map[string]interface{}
	if payloadJSON != "" {
		if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
			return `{"error":"invalid payload"}`
		}
	}

	op := msgType
	if v, ok := payload["op"].(string); ok {
		op = v
	}

	switch op {
	case "echo":
		a.EchoCount++
		result, _ := json.Marshal(map[string]interface{}{
			"ok": true, "echo": payload, "count": a.EchoCount,
		})
		return string(result)

	case "compute":
		p := 7
		if v, ok := payload["p"].(float64); ok {
			p = int(v)
		}
		isPrime := isMersennePrime(p)
		a.ComputeCount++
		result, _ := json.Marshal(map[string]interface{}{
			"ok": true, "p": p, "is_mersenne_prime": isPrime, "count": a.ComputeCount,
		})
		return string(result)

	case "kv_put":
		key := "perf_key"
		value := "perf_val"
		if v, ok := payload["key"].(string); ok {
			key = v
		}
		if v, ok := payload["value"].(string); ok {
			value = v
		}
		if err := host.KV().Put(key, value); err != nil {
			return `{"error":"` + err.Error() + `"}`
		}
		a.KvCount++
		result, _ := json.Marshal(map[string]interface{}{"ok": true, "key": key, "count": a.KvCount})
		return string(result)

	case "kv_get":
		key := "perf_key"
		if v, ok := payload["key"].(string); ok {
			key = v
		}
		val, err := host.KV().Get(key)
		if err != nil {
			return `{"error":"` + err.Error() + `"}`
		}
		result, _ := json.Marshal(map[string]interface{}{"ok": true, "key": key, "value": val})
		return string(result)

	case "pg_broadcast":
		group := "perf-group"
		if v, ok := payload["group"].(string); ok {
			group = v
		}
		if err := host.PG().Join(group); err != nil {
			return `{"error":"` + err.Error() + `"}`
		}
		msg := map[string]interface{}{"event": "ping"}
		if v, ok := payload["message"].(map[string]interface{}); ok {
			msg = v
		}
		msgJSON, _ := json.Marshal(msg)
		if err := host.PG().Broadcast(group, "perf_event", string(msgJSON)); err != nil {
			return `{"error":"` + err.Error() + `"}`
		}
		a.PgCount++
		result, _ := json.Marshal(map[string]interface{}{"ok": true, "group": group, "count": a.PgCount})
		return string(result)

	case "shard_task":
		shardIndex := 0
		if v, ok := payload["shard_index"].(float64); ok {
			shardIndex = int(v)
		}
		lr := 0.01
		if v, ok := payload["lr"].(float64); ok {
			lr = v
		}
		values := make([]float64, 100)
		for i := range values {
			values[i] = float64(i)
		}
		if rawVals, ok := payload["values"].([]interface{}); ok {
			values = make([]float64, len(rawVals))
			for i, v := range rawVals {
				if f, ok := v.(float64); ok {
					values[i] = f
				}
			}
		}
		stats := gradientStep(values, lr)
		a.ShardCount++
		stats["ok"] = true
		stats["shard_index"] = shardIndex
		stats["count"] = a.ShardCount
		result, _ := json.Marshal(stats)
		return string(result)

	case "get_stats":
		result, _ := json.Marshal(map[string]interface{}{
			"ok":            true,
			"echo_count":    a.EchoCount,
			"compute_count": a.ComputeCount,
			"kv_count":      a.KvCount,
			"pg_count":      a.PgCount,
			"shard_count":   a.ShardCount,
		})
		return string(result)

	default:
		return `{"error":"unknown op: ` + op + `"}`
	}
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("PerfActor", func() plexspaces.Actor { return newPerfActor() })
	plexspaces.Register(router)
}

func main() {}
