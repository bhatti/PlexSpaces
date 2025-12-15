//! Byzantine Generals Problem implementation for PlexSpaces
//!
//! This example demonstrates all 5 foundational pillars:
//! 1. TupleSpace for vote coordination
//! 2. OTP supervision for fault tolerance
//! 3. Journal for durable decisions
//! 4. WASM runtime for portable logic (phase 2)
//! 5. Firecracker VMs for isolation (future)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;

use plexspaces::ActorId;  // ActorId is re-exported from root
use plexspaces::{Actor, ActorContext, BehaviorType, BehaviorError};  // Re-exported from root
use plexspaces::mailbox::{Mailbox, MailboxConfig};
use plexspaces::journal::Journal;
use plexspaces::tuplespace::{Tuple, TupleField, Pattern, PatternField, TupleSpaceError, TupleSpace};
use plexspaces::tuplespace::lattice_space::LatticeTupleSpace;
use plexspaces::lattice::{Lattice, SetLattice, LWWLattice, ConsistencyLevel};
use plexspaces_proto::v1::tuplespace::TupleSpaceConfig;
use plexspaces_core::ActorError;

use serde::{Serialize, Deserialize};

// ============================================================================
// Configuration Helpers
// ============================================================================
//
// These functions create configured TupleSpace instances for Byzantine Generals tests

/// Create a TupleSpace instance from environment or default
///
/// ## Purpose
/// Provides a convenient way to create TupleSpace for Byzantine tests that respects
/// environment configuration (for multi-process tests) or falls back to in-memory.
///
/// ## Environment Variables
/// - `PLEXSPACES_TUPLESPACE_BACKEND`: Backend type ("in-memory", "sqlite", "redis", "postgres")
/// - `PLEXSPACES_SQLITE_PATH`: SQLite database file path
/// - Other vars per TupleSpace::from_env() documentation
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc<dyn TupleSpaceOps> for use with General
///
/// ## Examples
/// ```rust
/// # use byzantine_generals::create_tuplespace_from_env;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Uses env vars if set, otherwise in-memory
/// let tuplespace = create_tuplespace_from_env().await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_tuplespace_from_env() -> Result<Arc<dyn TupleSpaceOps>, Box<dyn std::error::Error>> {
    // Try environment variables first, fall back to in-memory
    let tuplespace = TupleSpace::from_env_or_default().await?;

    // Wrap in Arc<dyn TupleSpaceOps> for Byzantine API
    Ok(Arc::new(tuplespace))
}

/// Create a TupleSpace instance from explicit configuration
///
/// ## Purpose
/// Allows tests to explicitly specify backend configuration instead of relying on env vars.
/// Useful for integration tests that need specific backends.
///
/// ## Arguments
/// * `config` - TupleSpaceConfig protobuf message
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc<dyn TupleSpaceOps>
///
/// ## Examples
/// ```rust
/// # use byzantine_generals::create_tuplespace_from_config;
/// # use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, SqliteBackend};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = TupleSpaceConfig {
///     backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
///         SqliteBackend { path: ":memory:".to_string() }
///     )),
///     pool_size: 1,
///     default_ttl_seconds: 0,
///     enable_indexing: false,
/// };
/// let tuplespace = create_tuplespace_from_config(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_tuplespace_from_config(config: TupleSpaceConfig) -> Result<Arc<dyn TupleSpaceOps>, Box<dyn std::error::Error>> {
    let tuplespace = TupleSpace::from_config(config).await?;
    Ok(Arc::new(tuplespace))
}

/// Create an in-memory TupleSpace with lattice consistency
///
/// ## Purpose
/// Convenience function for creating in-memory TupleSpace with lattice-based consistency.
/// This is the default for single-process tests.
///
/// ## Arguments
/// * `node_id` - Node identifier for lattice operations
/// * `consistency` - Consistency level (Linearizable, Causal, Eventual)
///
/// ## Returns
/// In-memory LatticeTupleSpace wrapped in Arc<dyn TupleSpaceOps>
///
/// ## Examples
/// ```rust
/// # use byzantine_generals::create_lattice_tuplespace;
/// # use plexspaces::lattice::ConsistencyLevel;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let tuplespace = create_lattice_tuplespace("node1", ConsistencyLevel::Linearizable).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_lattice_tuplespace(
    node_id: &str,
    consistency: ConsistencyLevel,
) -> Result<Arc<dyn TupleSpaceOps>, Box<dyn std::error::Error>> {
    let lattice_space = LatticeTupleSpace::new(node_id.to_string(), consistency);
    Ok(Arc::new(lattice_space))
}

/// The decision that generals must agree upon
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Decision {
    Attack,
    Retreat,
    Undecided,
}

/// A vote from a general
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Vote {
    pub general_id: String,
    pub round: u32,
    pub value: Decision,
    pub path: Vec<String>,  // Chain of generals who relayed this vote
}

/// State of a general in the Byzantine army
#[derive(Debug, Clone)]
pub struct GeneralState {
    pub id: String,
    pub is_commander: bool,
    pub is_faulty: bool,
    pub round: u32,
    pub votes: HashMap<String, Vec<Vote>>,  // Votes received per round
    pub decision: Decision,
    pub message_count: usize,
}

/// Trait for TupleSpace operations needed by Byzantine Generals
///
/// This allows General to work with any TupleSpace implementation (LatticeTupleSpace, RedisTupleSpace, etc.)
#[async_trait]
pub trait TupleSpaceOps: Send + Sync {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError>;
    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError>;
}

/// Implement TupleSpaceOps for LatticeTupleSpace
#[async_trait]
impl TupleSpaceOps for LatticeTupleSpace {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        self.write(tuple).await
            .map_err(|e| TupleSpaceError::BackendError(e.to_string()))
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        // LatticeTupleSpace.read() doesn't return Result, just Vec<Tuple>
        let tuples = self.read(pattern).await;
        Ok(tuples)
    }
}

/// Implement TupleSpaceOps for RedisTupleSpace
#[cfg(feature = "redis-backend")]
#[async_trait]
impl TupleSpaceOps for plexspaces::tuplespace::RedisTupleSpace {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        self.write(tuple).await
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        self.read(pattern).await
    }
}

/// Implement TupleSpaceOps for regular TupleSpace (configurable backend)
///
/// This allows Byzantine example to work with any configured backend (SQLite, Redis, PostgreSQL)
#[async_trait]
impl TupleSpaceOps for TupleSpace {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        self.write(tuple).await
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        // TupleSpace.read() returns Result<Option<Tuple>> for single match
        // For Byzantine, we need read_all() which returns all matching tuples
        self.read_all(pattern.clone()).await
    }
}

/// A general in the Byzantine army
#[derive(Clone)]
pub struct General {
    pub id: ActorId,
    pub state: Arc<RwLock<GeneralState>>,
    pub mailbox: Arc<Mailbox>,
    pub journal: Arc<dyn Journal>,
    pub tuplespace: Arc<dyn TupleSpaceOps>,
}

impl General {
    /// Create a new general
    pub async fn new(
        id: String,
        is_commander: bool,
        is_faulty: bool,
        journal: Arc<dyn Journal>,
        tuplespace: Arc<dyn TupleSpaceOps>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let actor_id = id.clone();
        let state = GeneralState {
            id: id.clone(),
            is_commander,
            is_faulty,
            round: 0,
            votes: HashMap::new(),
            decision: Decision::Undecided,
            message_count: 0,
        };

        Ok(General {
            id: actor_id.clone(),
            state: Arc::new(RwLock::new(state)),
            mailbox: Arc::new(Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", actor_id)).await?),
            journal,
            tuplespace,
        })
    }

    /// Commander proposes a value
    pub async fn propose(&self, attack: bool) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.write().await;

        if !state.is_commander {
            return Err("Only commander can propose".into());
        }

        let decision = if attack { Decision::Attack } else { Decision::Retreat };
        state.decision = decision;

        // TODO: Update to new Journal API
        // Old API: self.journal.append(JournalEntry { ... })
        // New API: Use typed methods like record_state_change()
        // For now, commenting out to get compilation working

        // Write proposal to TupleSpace
        let tuple = Tuple::new(vec![
            TupleField::String("proposal".to_string()),
            TupleField::String(state.id.clone()),
            TupleField::Integer(0),  // Round 0
            TupleField::String(format!("{:?}", decision)),
        ]);
        self.tuplespace.write(tuple).await?;

        println!("Commander {} proposes: {:?}", state.id, decision);

        Ok(())
    }

    /// Cast a vote to the TupleSpace
    pub async fn cast_vote(&self, round: u32, value: Decision, path: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.state.read().await;

        // Create vote
        let vote = Vote {
            general_id: state.id.clone(),
            round,
            value,
            path: path.clone(),
        };

        // Write vote to TupleSpace
        let tuple = Tuple::new(vec![
            TupleField::String("vote".to_string()),
            TupleField::String(state.id.clone()),
            TupleField::Integer(round as i64),
            TupleField::String(format!("{:?}", value)),
            TupleField::String(path.join(",")),
        ]);
        self.tuplespace.write(tuple).await?;

        // TODO: Update to new Journal API
        // For now, commenting out to get compilation working

        println!("General {} votes {:?} in round {}", state.id, value, round);

        Ok(())
    }

    /// Read votes from TupleSpace for a given round
    pub async fn read_votes(&self, round: u32) -> Result<Vec<Vote>, Box<dyn std::error::Error>> {
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("vote".to_string())),
            PatternField::Wildcard,  // Any general
            PatternField::Exact(TupleField::Integer(round as i64)),
            PatternField::Wildcard,  // Any value
            PatternField::Wildcard,  // Any path
        ]);

        let tuples = self.tuplespace.read(&pattern).await?;

        let votes: Vec<Vote> = tuples.into_iter().filter_map(|t| {
            let fields = t.fields();

            // Extract fields using pattern matching
            let general_id = match &fields[1] {
                TupleField::String(s) => s.clone(),
                _ => return None,
            };

            let value_str = match &fields[3] {
                TupleField::String(s) => s.as_str(),
                _ => return None,
            };

            let value = match value_str {
                "Attack" => Decision::Attack,
                "Retreat" => Decision::Retreat,
                _ => Decision::Undecided,
            };

            let path = match &fields[4] {
                TupleField::String(s) => s.split(',').map(String::from).collect(),
                _ => Vec::new(),
            };

            Some(Vote {
                general_id,
                round,
                value,
                path,
            })
        }).collect();

        Ok(votes)
    }

    /// Process votes and reach a decision
    pub async fn decide(&self) -> Result<Decision, Box<dyn std::error::Error>> {
        let mut state = self.state.write().await;

        if state.is_commander {
            // Commander sticks to their proposal
            return Ok(state.decision);
        }

        // Collect all votes from all rounds
        let mut all_votes = Vec::new();
        for round in 0..=state.round {
            let votes = self.read_votes(round).await?;
            all_votes.extend(votes);
        }

        // Count votes (majority voting)
        let mut vote_counts: HashMap<Decision, usize> = HashMap::new();
        for vote in &all_votes {
            *vote_counts.entry(vote.value).or_insert(0) += 1;
        }

        // Find majority
        let total_votes: usize = vote_counts.values().sum();
        let majority_threshold = total_votes / 2;

        let decision = vote_counts
            .into_iter()
            .find(|(_, count)| *count > majority_threshold)
            .map(|(decision, _)| decision)
            .unwrap_or(Decision::Retreat);  // Default to retreat if no majority

        state.decision = decision;

        // TODO: Update to new Journal API
        // For now, commenting out to get compilation working

        println!("General {} decides: {:?}", state.id, decision);

        Ok(decision)
    }

    /// Simulate Byzantine behavior for faulty generals
    pub fn get_faulty_value(&self, original: Decision) -> Decision {
        // Faulty generals send opposite values
        match original {
            Decision::Attack => Decision::Retreat,
            Decision::Retreat => Decision::Attack,
            Decision::Undecided => Decision::Undecided,
        }
    }
}

#[async_trait]
impl Actor for General {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        msg: plexspaces_mailbox::Message,
    ) -> Result<(), BehaviorError> {
        let mut state = self.state.write().await;
        state.message_count += 1;

        match msg.message_type.as_str() {
            "PROPOSE" => {
                // Commander's proposal received
                if let Ok(decision) = bincode::deserialize::<Decision>(&msg.payload) {
                    println!("General {} received proposal: {:?}", state.id, decision);

                    // Relay the proposal (potentially modified if faulty)
                    let value = if state.is_faulty {
                        self.get_faulty_value(decision)
                    } else {
                        decision
                    };

                    drop(state);  // Release write lock
                    self.cast_vote(0, value, vec![self.state.read().await.id.clone()])
                        .await
                        .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                }
            }
            "VOTE" => {
                // Process incoming vote
                if let Ok(vote) = bincode::deserialize::<Vote>(&msg.payload) {
                    state.votes
                        .entry(vote.round.to_string())
                        .or_insert_with(Vec::new)
                        .push(vote.clone());

                    // Relay vote in next round if needed
                    if vote.round < 2 {  // Limit rounds
                        let mut new_path = vote.path.clone();
                        new_path.push(state.id.clone());

                        let value = if state.is_faulty {
                            self.get_faulty_value(vote.value)
                        } else {
                            vote.value
                        };

                        drop(state);  // Release write lock
                        self.cast_vote(vote.round + 1, value, new_path)
                            .await
                            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                    }
                }
            }
            "DECIDE" => {
                drop(state);  // Release write lock
                self.decide()
                    .await
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
            }
            _ => {
                println!("General {} received unknown message type: {}",
                         state.id, msg.message_type);
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    // NOTE: pre_start/post_stop lifecycle hooks are now static on Actor,
    // not part of Actor trait. They would be called via:
    // Actor::on_activate() and Actor::on_deactivate()
}

/// Supervisor for Byzantine generals
///
/// TODO: Update to new Supervision API
/// Old pattern: Implement Supervisor trait
/// New pattern: Use Supervisor struct with configuration
///
/// For now, keeping this as a simple coordinator struct.
/// Actual supervision would be done via:
///   let supervisor = Supervisor::new("byzantine-supervisor", strategy).await?;
///   supervisor.add_child(actor_spec).await?;
pub struct ByzantineSupervisor {
    pub num_generals: usize,
    pub num_faulty: usize,
}

impl ByzantineSupervisor {
    pub fn new(num_generals: usize, num_faulty: usize) -> Self {
        ByzantineSupervisor {
            num_generals,
            num_faulty,
        }
    }
}

/// Create ActorSpec for a Byzantine general
///
/// ## Purpose
/// Factory function to create supervised general actors with specified properties.
/// This is used by supervised_consensus tests.
///
/// ## Arguments
/// * `id` - Unique identifier for this general (e.g., "general-0@localhost")
/// * `is_commander` - Whether this is the commanding general (proposes initial value)
/// * `is_faulty` - Whether this general exhibits Byzantine behavior (sends conflicting votes)
/// * `tuplespace` - TupleSpace instance for vote coordination
/// * `journal` - Journal for durable decision recording
///
/// ## Returns
/// ActorSpec ready to be added to a Supervisor
///
/// ## Examples
/// ```rust,no_run
/// use std::sync::Arc;
/// use byzantine_generals::{create_general_spec, create_lattice_tuplespace};
/// use plexspaces::journal::MemoryJournal;
/// use plexspaces::lattice::ConsistencyLevel;
/// use plexspaces::supervisor::Supervisor;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let tuplespace = create_lattice_tuplespace("node1", ConsistencyLevel::Linearizable).await?;
/// let journal = Arc::new(MemoryJournal::new());
///
/// let spec = create_general_spec(
///     "general-0@localhost".to_string(),
///     true,  // is_commander
///     false, // is_faulty
///     tuplespace,
///     journal,
/// );
///
/// // Add to supervisor
/// // let (supervisor, _) = Supervisor::new("supervisor".to_string(), strategy);
/// // supervisor.add_child(spec).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Implementation Notes
/// - General already implements Actor trait
/// - Factory creates a fresh General instance on each call (no Clone needed)
/// - Each restart creates a new General with same parameters
/// - Mailbox is created fresh for each restart
pub fn create_general_spec(
    id: String,
    is_commander: bool,
    is_faulty: bool,
    tuplespace: Arc<dyn TupleSpaceOps>,
    journal: Arc<dyn Journal>,
) -> plexspaces_supervisor::ActorSpec {
    use plexspaces_actor::Actor;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};
    use plexspaces_supervisor::{ActorSpec, RestartPolicy, ChildType};

    // Clone Arc references for factory closure
    let ts = tuplespace.clone();
    let j = journal.clone();

    ActorSpec {
        id: id.clone(),
        factory: Arc::new(move || {
            // Create fresh General behavior instance
            // This is called on each restart, so General gets a clean state
            // Note: Factory is sync, but General::new is async - need to use block_on
            let rt = tokio::runtime::Runtime::new().unwrap();
            let general = match rt.block_on(General::new(
                id.clone(),
                is_commander,
                is_faulty,
                j.clone(),
                ts.clone(),
            )) {
                Ok(g) => g,
                Err(e) => return Err(plexspaces_core::ActorError::InvalidState(format!("Failed to create General: {}", e))),
            };

            // Wrap General in Actor
            // General implements Actor, so this is type-safe
            let mailbox = match rt.block_on(Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", id.clone()))) {
                Ok(m) => m,
                Err(e) => return Err(plexspaces_core::ActorError::InvalidState(format!("Failed to create Mailbox: {}", e))),
            };
            Ok(Actor::new(
                id.clone(),
                Box::new(general),  // Box<dyn Actor>
                mailbox,
                "byzantine".to_string(), // namespace for journal entries
                None, // node_id - will be set when spawned
            ))
        }),
        restart: RestartPolicy::Permanent,  // Always restart on failure
        child_type: ChildType::Worker,      // Regular actor (not supervisor)
        shutdown_timeout_ms: Some(5000),    // 5 seconds for graceful shutdown
    }
}

// Old Supervisor trait implementation removed - Supervisor is now a struct, not a trait
// See API_CHANGES.md for details on the new supervision pattern

/// Lattice-based consensus state for coordination-free agreement
#[derive(Debug, Clone, PartialEq)]
pub struct ConsensusLattice {
    pub votes: SetLattice<Vote>,
    pub decision: LWWLattice<Decision>,
}

impl ConsensusLattice {
    pub fn new(node_id: String) -> Self {
        ConsensusLattice {
            votes: SetLattice::new(),
            decision: LWWLattice::new(Decision::Undecided, 0, node_id),
        }
    }

    pub fn add_vote(&mut self, vote: Vote) {
        self.votes = self.votes.merge(&SetLattice::singleton(vote));
    }

    pub fn update_decision(&mut self, decision: Decision, timestamp: u64, node_id: String) {
        let new_decision = LWWLattice::new(decision, timestamp, node_id);
        self.decision = self.decision.merge(&new_decision);
    }

    pub fn get_consensus(&self) -> Decision {
        // Count votes using the set
        let mut counts: HashMap<Decision, usize> = HashMap::new();
        for vote in &self.votes.elements {
            *counts.entry(vote.value).or_insert(0) += 1;
        }

        // Find majority
        let total: usize = counts.values().sum();
        counts.into_iter()
            .find(|(_, count)| *count > total / 2)
            .map(|(decision, _)| decision)
            .unwrap_or(self.decision.value)
    }
}

impl Lattice for ConsensusLattice {
    fn merge(&self, other: &Self) -> Self {
        ConsensusLattice {
            votes: self.votes.merge(&other.votes),
            decision: self.decision.merge(&other.decision),
        }
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.votes.subsumes(&other.votes) &&
        self.decision.subsumes(&other.decision)
    }

    fn bottom() -> Self {
        ConsensusLattice {
            votes: SetLattice::bottom(),
            decision: LWWLattice::new(Decision::Undecided, 0, "bottom".to_string()),
        }
    }
}