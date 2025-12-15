// Gosiris Counter Actor (Go)
// Demonstrates Go actor framework with message passing

package main

import (
    "github.com/teivah/gosiris"
    "context"
)

type CounterActor struct {
    count int
}

func (a *CounterActor) Receive(ctx context.Context, message interface{}) {
    switch msg := message.(type) {
    case Increment:
        a.count++
        ctx.Send(msg.Sender, Count(a.count))
    case Decrement:
        a.count = max(0, a.count-1)
        ctx.Send(msg.Sender, Count(a.count))
    case Get:
        ctx.Send(msg.Sender, Count(a.count))
    }
}

type Increment struct {
    Sender gosiris.ActorRef
}

type Decrement struct {
    Sender gosiris.ActorRef
}

type Get struct {
    Sender gosiris.ActorRef
}

type Count int

func main() {
    system := gosiris.NewActorSystem()
    counter := system.ActorOf("counter", &CounterActor{count: 0})
    
    // Send messages
    system.Tell(counter, Increment{Sender: system.Self()})
    system.Tell(counter, Get{Sender: system.Self()})
}
