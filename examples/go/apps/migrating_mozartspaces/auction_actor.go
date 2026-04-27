// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// MozartSpaces → PlexSpaces: Distributed auction (Go WASM)
//
// Real-world use case: Real-time bidding with TupleSpace (bids), process group
// (broadcast to bidders), and distributed lock (commit winner). XVSM/Linda-style coordination.
//
// Abstractions: host.TS() (tuple space), host.PG() (process group), host.LockAcquire/LockRelease.

package main

import (
	"encoding/json"
	"strconv"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

const (
	bidPrefix     = "bid"
	auctionPrefix = "auction"
	tenantID      = "default"
	namespace     = "auction"
	lockLeaseSec  = 30
	lockTimeoutMs = 5000
	gatherPollMax = 500
)

// AuctionActor runs an auction: scatter open tuple, gather bids from tuple space,
// acquire lock to commit winner, broadcast sold. Handles place_bid to inject bids.
type AuctionActor struct {
	plexspaces.BaseActor

	AuctionID          string  `json:"auction_id"`
	ReservePrice       float64 `json:"reserve_price"`
	MaxBids            int     `json:"max_bids"`
	Status             string  `json:"status"`
	WinnerID           string  `json:"winner_id"`
	WinningAmount      float64 `json:"winning_amount"`
	BidsProcessed      int     `json:"bids_processed"`
	TotalComputeMs     float64 `json:"total_compute_ms"`
	TotalCoordMs       float64 `json:"total_coord_ms"`
	CreatedAtMs        uint64  `json:"created_at_ms"`
	UpdatedAtMs        uint64  `json:"updated_at_ms"`
	CancelRequested    bool    `json:"cancel_requested"`
	BiddersJoined      bool    `json:"bidders_joined"`
}

func NewAuctionActor() *AuctionActor {
	a := &AuctionActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (a *AuctionActor) Init(configJSON string) string {
	return ""
}

func (a *AuctionActor) Run(payloadJSON string) string {
	t0 := host.NowMs()
	if a.CreatedAtMs == 0 {
		a.CreatedAtMs = t0
	}
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	a.UpdatedAtMs = host.NowMs()

	if a.CancelRequested {
		a.Status = "cancelled"
		return a.finish(t0, 0, "cancelled")
	}
	if a.Status == "completed" || a.Status == "sold" {
		return a.finish(t0, 0, a.Status)
	}

	auctionID := ""
	if id, ok := payload["auction_id"].(string); ok && id != "" {
		auctionID = id
	}
	if auctionID == "" {
		return a.finish(t0, 0, "no_auction_id")
	}
	a.AuctionID = auctionID

	reserve := 0.0
	if r, ok := payload["reserve_price"].(float64); ok {
		reserve = r
	}
	a.ReservePrice = reserve

	maxBids := 100
	if n, ok := payload["max_bids"].(float64); ok && n > 0 {
		maxBids = int(n)
	} else if n, ok := payload["max_bids"].(int); ok && n > 0 {
		maxBids = n
	}
	a.MaxBids = maxBids
	a.Status = "open"
	a.WinnerID = ""
	a.WinningAmount = reserve
	a.BidsProcessed = 0

	// Scatter: write open tuple
	host.TS().Write([]any{auctionPrefix, auctionID, "open", reserve})

	// Broadcast to bidders (join PG if not already)
	pgName := "auction:" + auctionID + ":bidders"
	if !a.BiddersJoined {
		_ = host.PG().Join(pgName)
		a.BiddersJoined = true
	}
	_ = host.PG().Broadcast(pgName, "auction_start", map[string]any{
		"auction_id": auctionID,
		"reserve":    reserve,
		"max_bids":   maxBids,
	})

	// Gather: take bids from tuple space until max_bids or no more
	pattern := []any{bidPrefix, auctionID, nil, nil, nil}
	computeMs := 0.0
	for i := 0; i < gatherPollMax; i++ {
		if a.CancelRequested {
			a.Status = "cancelled"
			return a.finish(t0, computeMs, "cancelled")
		}
		tuple, ok := host.TS().Take(pattern)
		if !ok || len(tuple) < 5 {
			if a.BidsProcessed > 0 {
				break
			}
			a.UpdatedAtMs = host.NowMs()
			continue
		}
		bidderID, _ := tuple[2].(string)
		amount := toFloat64(tuple[4])
		computeMs += 0.5
		if amount >= a.WinningAmount {
			a.WinningAmount = amount
			a.WinnerID = bidderID
			_ = host.PG().Broadcast(pgName, "new_bid", map[string]any{
				"bidder_id": bidderID,
				"amount":    amount,
			})
		}
		a.BidsProcessed++
		if a.BidsProcessed >= maxBids {
			break
		}
	}

	// Commit: acquire lock, write sold tuple, broadcast, release
	lockName := "auction:" + auctionID + ":commit"
	holderID := host.SelfID()
	lockResult := host.LockAcquire(tenantID, namespace, holderID, lockName, lockLeaseSec, lockTimeoutMs)

	if lockResult != "" && !isHostError(lockResult) {
		var lockInfo struct {
			LockKey string `json:"lock_key"`
			Version string `json:"version"`
		}
		_ = json.Unmarshal([]byte(lockResult), &lockInfo)
		host.TS().Write([]any{auctionPrefix, auctionID, "sold", a.WinnerID, a.WinningAmount})
		_ = host.PG().Broadcast(pgName, "sold", map[string]any{
			"winner_id": a.WinnerID,
			"amount":    a.WinningAmount,
		})
		host.LockRelease(lockInfo.LockKey, tenantID, namespace, holderID, lockInfo.Version)
		a.Status = "sold"
	}
	a.UpdatedAtMs = host.NowMs()

	return a.finish(t0, computeMs, a.Status)
}

func (a *AuctionActor) finish(t0 uint64, computeMs float64, status string) string {
	elapsed := float64(host.NowMs() - t0)
	coordMs := elapsed - computeMs
	if coordMs < 0 {
		coordMs = 0
	}
	a.TotalComputeMs += computeMs
	a.TotalCoordMs += coordMs
	a.UpdatedAtMs = host.NowMs()
	return marshal(map[string]any{
		"status":           status,
		"auction_id":       a.AuctionID,
		"winner_id":        a.WinnerID,
		"winning_amount":   a.WinningAmount,
		"bids_processed":   a.BidsProcessed,
		"total_compute_ms": a.TotalComputeMs,
		"total_coord_ms":   a.TotalCoordMs,
	})
}

func (a *AuctionActor) Signal(name, _ string) {
	if name == "cancel" {
		a.CancelRequested = true
		a.UpdatedAtMs = host.NowMs()
	}
}

func (a *AuctionActor) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"auction_id":        a.AuctionID,
			"status":            a.Status,
			"winner_id":         a.WinnerID,
			"winning_amount":    a.WinningAmount,
			"bids_processed":    a.BidsProcessed,
			"cancel_requested":   a.CancelRequested,
			"total_compute_ms":  a.TotalComputeMs,
			"total_coord_ms":    a.TotalCoordMs,
			"created_at_ms":     a.CreatedAtMs,
			"updated_at_ms":     a.UpdatedAtMs,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

// Handle processes place_bid (write bid tuple) and join (join process group).
func (a *AuctionActor) Handle(from, msgType, payloadJSON string) string {
	switch msgType {
	case "place_bid":
		var payload map[string]any
		_ = json.Unmarshal([]byte(payloadJSON), &payload)
		auctionID, _ := payload["auction_id"].(string)
		bidderID, _ := payload["bidder_id"].(string)
		amount := 0.0
		if amt, ok := payload["amount"].(float64); ok {
			amount = amt
		}
		if auctionID == "" || bidderID == "" {
			return marshal(map[string]any{"error": "missing auction_id or bidder_id"})
		}
		ts := host.NowMs()
		out := host.TS().Write([]any{bidPrefix, auctionID, bidderID, float64(ts), amount})
		if out != "" && len(out) >= 5 && out[:5] == "ERROR" {
			return marshal(map[string]any{"error": out})
		}
		return marshal(map[string]any{"ok": true, "bidder_id": bidderID, "amount": amount})
	case "join":
		var payload map[string]any
		_ = json.Unmarshal([]byte(payloadJSON), &payload)
		auctionID, _ := payload["auction_id"].(string)
		if auctionID != "" {
			_ = host.PG().Join("auction:" + auctionID + ":bidders")
			a.BiddersJoined = true
		}
		return marshal(map[string]any{"ok": true})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func marshal(v map[string]any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func toFloat64(v any) float64 {
	switch x := v.(type) {
	case float64:
		return x
	case int:
		return float64(x)
	case string:
		f, _ := strconv.ParseFloat(x, 64)
		return f
	}
	return 0
}

func isHostError(s string) bool {
	return len(s) >= 5 && (s[:5] == "ERROR" || s[:5] == "error")
}

func init() {
	plexspaces.Register(NewAuctionActor())
}

func main() {}
