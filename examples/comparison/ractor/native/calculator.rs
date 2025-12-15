// Ractor Calculator Actor (Rust)
// Demonstrates Rust-native actors with type-safe message passing

use ractor::{Actor, ActorRef, Message, RpcReplyPort};

#[derive(Debug)]
pub enum CalculatorMessage {
    Add { a: f64, b: f64, reply: RpcReplyPort<f64> },
    Subtract { a: f64, b: f64, reply: RpcReplyPort<f64> },
    Multiply { a: f64, b: f64, reply: RpcReplyPort<f64> },
    Divide { a: f64, b: f64, reply: RpcReplyPort<f64> },
}

impl Message for CalculatorMessage {}

pub struct CalculatorActor;

#[async_trait::async_trait]
impl Actor for CalculatorActor {
    type Msg = CalculatorMessage;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> ActorResult<()> {
        match message {
            CalculatorMessage::Add { a, b, reply } => {
                let _ = reply.send(a + b);
            }
            CalculatorMessage::Subtract { a, b, reply } => {
                let _ = reply.send(a - b);
            }
            CalculatorMessage::Multiply { a, b, reply } => {
                let _ = reply.send(a * b);
            }
            CalculatorMessage::Divide { a, b, reply } => {
                let _ = reply.send(a / b);
            }
        }
        Ok(())
    }
}

// Usage:
// let (actor, handle) = Actor::spawn(None, CalculatorActor, ()).await?;
// let result = actor.call(CalculatorMessage::Add { a: 10.0, b: 5.0, reply: port }).await?;
