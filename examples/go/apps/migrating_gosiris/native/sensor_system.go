// Native Gosiris reference implementation
//
// This shows how the same IoT sensor aggregation would be built
// using the Gosiris actor framework (github.com/teivah/gosiris).
//
// Run: go run sensor_system.go
//
// Note: This is a reference implementation for comparison purposes.
// The PlexSpaces version adds: process groups, supervision, state persistence,
// and distributed deployment without code changes.

package main

import (
	"context"
	"fmt"
	"math"
	"math/rand"
	"sync"
	"time"
)

// ========================================================================
// Gosiris-style Actor System (simplified)
// ========================================================================

type ActorRef struct {
	Name    string
	mailbox chan Message
}

type Message struct {
	Type    string
	Payload interface{}
	Sender  *ActorRef
}

type Actor interface {
	Receive(ctx context.Context, msg Message)
}

type ActorSystem struct {
	actors map[string]*ActorRef
	impls  map[string]Actor
	mu     sync.RWMutex
}

func NewActorSystem() *ActorSystem {
	return &ActorSystem{
		actors: make(map[string]*ActorRef),
		impls:  make(map[string]Actor),
	}
}

func (s *ActorSystem) ActorOf(name string, actor Actor) *ActorRef {
	ref := &ActorRef{
		Name:    name,
		mailbox: make(chan Message, 100),
	}
	s.mu.Lock()
	s.actors[name] = ref
	s.impls[name] = actor
	s.mu.Unlock()

	go func() {
		ctx := context.Background()
		for msg := range ref.mailbox {
			actor.Receive(ctx, msg)
		}
	}()
	return ref
}

func (s *ActorSystem) Tell(ref *ActorRef, msg Message) {
	ref.mailbox <- msg
}

func (s *ActorSystem) Ask(ref *ActorRef, msg Message) Message {
	replyCh := make(chan Message, 1)
	replyRef := &ActorRef{Name: "reply", mailbox: replyCh}
	msg.Sender = replyRef
	ref.mailbox <- msg

	select {
	case reply := <-replyCh:
		return reply
	case <-time.After(5 * time.Second):
		return Message{Type: "error", Payload: "timeout"}
	}
}

// ========================================================================
// Sensor Actor (Gosiris style)
// ========================================================================

type SensorReading struct {
	SensorID  string
	TempC     float64
	Humidity  float64
	Timestamp time.Time
}

type SensorActor struct {
	sensorID   string
	location   string
	baseTemp   float64
	baseHumid  float64
	readings   []SensorReading
	readCount  int
	rng        *rand.Rand
}

func NewSensorActor(id, location string, baseTemp, baseHumid float64) *SensorActor {
	return &SensorActor{
		sensorID:  id,
		location:  location,
		baseTemp:  baseTemp,
		baseHumid: baseHumid,
		readings:  make([]SensorReading, 0),
		rng:       rand.New(rand.NewSource(time.Now().UnixNano())),
	}
}

func (s *SensorActor) Receive(_ context.Context, msg Message) {
	switch msg.Type {
	case "read":
		s.readCount++
		drift := math.Sin(float64(s.readCount)*0.01) * 2.0
		reading := SensorReading{
			SensorID:  s.sensorID,
			TempC:     s.baseTemp + drift + (s.rng.Float64()-0.5),
			Humidity:  s.baseHumid + drift*1.5 + (s.rng.Float64()-0.5)*2,
			Timestamp: time.Now(),
		}
		s.readings = append(s.readings, reading)
		if len(s.readings) > 500 {
			s.readings = s.readings[len(s.readings)-500:]
		}
		if msg.Sender != nil {
			msg.Sender.mailbox <- Message{Type: "reading", Payload: reading}
		}

	case "stats":
		if msg.Sender != nil {
			msg.Sender.mailbox <- Message{
				Type: "sensor_stats",
				Payload: map[string]interface{}{
					"sensor_id":  s.sensorID,
					"location":   s.location,
					"read_count": s.readCount,
					"history":    len(s.readings),
				},
			}
		}
	}
}

// ========================================================================
// Aggregator Actor (Gosiris style)
// ========================================================================

type AggregatorActor struct {
	readings map[string][]SensorReading
	sensors  []*ActorRef
	system   *ActorSystem
}

func NewAggregatorActor(system *ActorSystem) *AggregatorActor {
	return &AggregatorActor{
		readings: make(map[string][]SensorReading),
		sensors:  make([]*ActorRef, 0),
		system:   system,
	}
}

func (a *AggregatorActor) RegisterSensor(ref *ActorRef) {
	a.sensors = append(a.sensors, ref)
}

func (a *AggregatorActor) Receive(_ context.Context, msg Message) {
	switch msg.Type {
	case "poll_all":
		for _, sensor := range a.sensors {
			reply := a.system.Ask(sensor, Message{Type: "read"})
			if reading, ok := reply.Payload.(SensorReading); ok {
				a.readings[reading.SensorID] = append(
					a.readings[reading.SensorID], reading,
				)
			}
		}
		if msg.Sender != nil {
			msg.Sender.mailbox <- Message{
				Type:    "poll_result",
				Payload: fmt.Sprintf("Polled %d sensors", len(a.sensors)),
			}
		}

	case "network_stats":
		totalReadings := 0
		for _, rs := range a.readings {
			totalReadings += len(rs)
		}
		if msg.Sender != nil {
			msg.Sender.mailbox <- Message{
				Type: "stats",
				Payload: map[string]interface{}{
					"sensors":        len(a.sensors),
					"total_readings": totalReadings,
				},
			}
		}
	}
}

// ========================================================================
// Main - Run the sensor network
// ========================================================================

func main() {
	fmt.Println("================================================================")
	fmt.Println("  Gosiris IoT Sensor Network (Native Go Reference)")
	fmt.Println("================================================================")
	fmt.Println()

	system := NewActorSystem()

	// Create sensors
	sensors := []struct {
		id, location string
		temp, humid  float64
	}{
		{"sensor-dc-zone-a", "dc-zone-a", 18.5, 35.0},
		{"sensor-dc-zone-b", "dc-zone-b", 24.0, 42.0},
		{"sensor-server-room", "server-room", 28.5, 30.0},
		{"sensor-outdoor", "outdoor", 15.0, 65.0},
	}

	agg := NewAggregatorActor(system)
	aggRef := system.ActorOf("aggregator", agg)

	for _, s := range sensors {
		actor := NewSensorActor(s.id, s.location, s.temp, s.humid)
		ref := system.ActorOf(s.id, actor)
		agg.RegisterSensor(ref)
		fmt.Printf("  Created sensor: %s (location=%s, base_temp=%.1fC)\n",
			s.id, s.location, s.temp)
	}
	fmt.Println()

	// Poll sensors multiple rounds
	numRounds := 50
	start := time.Now()

	for round := 1; round <= numRounds; round++ {
		reply := system.Ask(aggRef, Message{Type: "poll_all"})
		if round%10 == 0 {
			fmt.Printf("  Round %d: %v\n", round, reply.Payload)
		}
	}

	elapsed := time.Since(start)
	fmt.Printf("\n  Completed %d polling rounds in %v\n", numRounds, elapsed)

	// Get final stats
	statsReply := system.Ask(aggRef, Message{Type: "network_stats"})
	fmt.Printf("  Network stats: %v\n", statsReply.Payload)

	fmt.Println()
	fmt.Println("================================================================")
	fmt.Println("  Gosiris Reference Complete")
	fmt.Println("================================================================")
}
