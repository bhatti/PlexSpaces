// SPDX-License-Identifier: AGPL-3.0-or-later

package plexspaces

import (
	"encoding/json"
	"testing"
)

type abstractionsWorkflow struct {
	BaseActor
	Status  string   `json:"status"`
	Signals []string `json:"signals"`
}

type abstractionsChannel struct {
	BaseActor
	Received []map[string]string `json:"received"`
}

func newAbstractionsWorkflow() *abstractionsWorkflow {
	workflow := &abstractionsWorkflow{Status: "pending"}
	workflow.SetSelf(workflow)
	return workflow
}

func newAbstractionsChannel() *abstractionsChannel {
	channel := &abstractionsChannel{}
	channel.SetSelf(channel)
	return channel
}

func (w *abstractionsWorkflow) Init(configJSON string) string { return "" }
func (w *abstractionsWorkflow) Handle(from, msgType, payloadJSON string) string {
	return `{"error":"unexpected"}`
}
func (w *abstractionsWorkflow) Run(payloadJSON string) string {
	var payload struct {
		OrderID string `json:"order_id"`
	}
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	w.Status = "running:" + payload.OrderID
	data, _ := json.Marshal(map[string]any{"status": w.Status})
	return string(data)
}
func (w *abstractionsWorkflow) Signal(name, payloadJSON string) {
	var payload struct {
		Reason string `json:"reason"`
	}
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	w.Signals = append(w.Signals, name+":"+payload.Reason)
	w.Status = "cancelled"
}
func (w *abstractionsWorkflow) Query(name, payloadJSON string) string {
	data, _ := json.Marshal(map[string]any{
		"name":    name,
		"status":  w.Status,
		"signals": w.Signals,
	})
	return string(data)
}

func (c *abstractionsChannel) Init(configJSON string) string { return "" }
func (c *abstractionsChannel) Handle(from, msgType, payloadJSON string) string {
	var payload struct {
		Channel string `json:"channel"`
		Body    string `json:"body"`
	}
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	if msgType == "publish" {
		c.Received = append(c.Received, map[string]string{
			"channel": payload.Channel,
			"body":    payload.Body,
		})
	}
	return "{}"
}

func TestAbstractionsHostContract(t *testing.T) {
	ResetStubs()
	h := NewHost()

	if result := h.KVPut("abstractions/config", "ready"); result != "" {
		t.Fatalf("KVPut() = %q", result)
	}
	if tupleResult := h.TS().Write([]any{"abstractions", "task", "t-1"}); tupleResult != "" {
		t.Fatalf("TS().Write() = %q", tupleResult)
	}
	if tupleValue, ok := h.TS().Read([]any{"abstractions", "task", nil}); !ok || len(tupleValue) != 3 {
		t.Fatalf("TS().Read() = %#v, %v", tupleValue, ok)
	}
	if result := h.BlobUpload("abstractions/blob-1", "aGVsbG8=", "text/plain"); result != "" {
		t.Fatalf("BlobUpload() = %q", result)
	}
	if err := h.PG().Join("abstractions-group"); err != nil {
		t.Fatalf("PG().Join() error = %v", err)
	}
	if result := h.Send("abstractions-channel", "publish", map[string]any{"channel": "alerts", "body": "direct"}); result != "" {
		t.Fatalf("Send() = %q", result)
	}
	if err := h.PG().Broadcast("abstractions-group", "notify", map[string]any{"ok": true}); err != nil {
		t.Fatalf("PG().Broadcast() error = %v", err)
	}
	timerID := h.SendAfter(100, "tick", map[string]any{"kind": "timer"})
	if timerID == "" {
		t.Fatal("SendAfter() returned empty timer ID")
	}
	spawnedID, err := h.Spawn("abstractions", "abstractions-actor", map[string]any{"count": 1})
	if err != nil {
		t.Fatalf("Spawn() error = %v", err)
	}
	if err := h.Stop(spawnedID); err != nil {
		t.Fatalf("Stop() error = %v", err)
	}

	if got := h.KVGet("abstractions/config"); got != "ready" {
		t.Fatalf("KVGet() = %q", got)
	}
	if got := h.BlobDownload("abstractions/blob-1"); got != "aGVsbG8=" {
		t.Fatalf("BlobDownload() = %q", got)
	}
	if members, err := h.PG().Members("abstractions-group"); err != nil || len(members) != 1 {
		t.Fatalf("PG().Members() = %#v, %v", members, err)
	}
	if sent := GetStubSentMessages(); len(sent) != 1 || sent[0].To != "abstractions-channel" || sent[0].MsgType != "publish" {
		t.Fatalf("GetStubSentMessages() = %#v", sent)
	}
	if groupSent := GetStubGroupMessages(); len(groupSent) != 1 || groupSent[0].Group != "abstractions-group" || groupSent[0].MsgType != "notify" {
		t.Fatalf("GetStubGroupMessages() = %#v", groupSent)
	}
}

func TestAbstractionsWorkflowContract(t *testing.T) {
	workflow := newAbstractionsWorkflow()

	runResult := workflow.Run(`{"order_id":"o-1"}`)
	if runResult != `{"status":"running:o-1"}` {
		t.Fatalf("Run() = %q", runResult)
	}
	workflow.Signal("cancel", `{"reason":"user"}`)
	queryResult := workflow.Query("status", `{}`)
	if queryResult != `{"name":"status","signals":["cancel:user"],"status":"cancelled"}` &&
		queryResult != `{"name":"status","status":"cancelled","signals":["cancel:user"]}` {
		t.Fatalf("Query() = %q", queryResult)
	}
}

func TestAbstractionsChannelContract(t *testing.T) {
	definition := EventActor(func() Actor { return newAbstractionsChannel() }, "process_group")
	if definition.BehaviorType != BehaviorEventActor {
		t.Fatalf("BehaviorType = %q", definition.BehaviorType)
	}
	if len(definition.Facets) != 1 || definition.Facets[0] != "process_group" {
		t.Fatalf("Facets = %#v", definition.Facets)
	}

	channel := newAbstractionsChannel()
	if got := channel.Handle("publisher", "publish", `{"channel":"alerts","body":"hello"}`); got != "{}" {
		t.Fatalf("Handle() = %q", got)
	}
	if len(channel.Received) != 1 || channel.Received[0]["channel"] != "alerts" || channel.Received[0]["body"] != "hello" {
		t.Fatalf("Received = %#v", channel.Received)
	}
}
