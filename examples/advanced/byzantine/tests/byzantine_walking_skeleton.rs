//! Byzantine Generals Walking Skeleton - TDD Approach
//!
//! Building end-to-end functionality incrementally with tests first.

use plexspaces::actor::Actor;
use plexspaces::ActorId;  // ActorId is re-exported from root
use plexspaces::{Actor, BehaviorError, BehaviorType};  // Re-exported from root
use plexspaces::mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use tokio;

// ============================================================================
// PHASE 1: Minimal General Actor
// ============================================================================

/// Decision a general can make
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Attack,
    Retreat,
}

/// Simple General behavior for Byzantine consensus
pub struct GeneralBehavior {
    decision: Option<Decision>,
}

impl GeneralBehavior {
    pub fn new() -> Self {
        Self { decision: None }
    }

    pub fn get_decision(&self) -> Option<Decision> {
        self.decision.clone()
    }
}

#[async_trait::async_trait]
impl Actor for GeneralBehavior {
    async fn handle_message(&mut self, _ctx: &plexspaces_core::ActorContext, _msg: plexspaces_mailbox::Message) -> Result<(), BehaviorError> {
        // For now, just accept messages without processing
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("ByzantineGeneral".to_string())
    }
}

// ============================================================================
// TESTS - Following TDD Philosophy
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST 1: Can create a General actor
    /// This is the absolute minimum - just verify we can create an actor
    #[tokio::test]
    async fn test_can_create_general() {
        let actor_id: ActorId = "general_0".to_string();
        let behavior = Box::new(GeneralBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default());

        let general = Actor::new(
            actor_id.clone(),
            behavior,
            mailbox,
            "test".to_string(),
            None, // node_id
        );

        assert_eq!(general.id(), "general_0");
    }

    /// TEST 2: General can make a decision
    /// Verify the basic decision-making capability
    #[tokio::test]
    async fn test_general_can_make_decision() {
        let mut behavior = GeneralBehavior::new();

        // Initially, no decision
        assert_eq!(behavior.get_decision(), None);

        // Make a decision (we'll implement this in next iteration)
        behavior.decision = Some(Decision::Attack);

        // Verify decision is set
        assert_eq!(behavior.get_decision(), Some(Decision::Attack));
    }
}
