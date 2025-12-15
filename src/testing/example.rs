// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Trait for testable examples that can run in any environment
//!
//! This allows examples to be tested consistently across in-process,
//! Docker, Kubernetes, and Firecracker environments.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// Re-exported from core crate
use super::{ActorConfig, ActorHandle, TestEnvironment, TestError};

/// Result of running an example
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleResult {
    pub success: bool,
    pub duration: Duration,
    pub actors_deployed: usize,
    pub messages_sent: u64,
    pub custom_metrics: HashMap<String, f64>,
    pub errors: Vec<String>,
}

/// Configuration for running an example
#[derive(Debug, Clone)]
pub struct ExampleConfig {
    pub name: String,
    pub actors: Vec<ActorConfig>,
    pub parameters: HashMap<String, String>,
    pub timeout: Duration,
}

/// Trait that examples must implement to be testable
#[async_trait]
pub trait TestableExample: Send + Sync {
    /// Name of the example
    fn name(&self) -> &str;

    /// Description of what this example demonstrates
    fn description(&self) -> &str;

    /// Get the configuration for this example
    fn config(&self) -> ExampleConfig;

    /// Deploy actors for this example in the given environment
    async fn deploy_actors(
        &self,
        env: &dyn TestEnvironment,
        config: &ExampleConfig,
    ) -> Result<Vec<ActorHandle>, TestError>;

    /// Run the example logic
    async fn run(
        &self,
        env: &dyn TestEnvironment,
        actors: Vec<ActorHandle>,
        config: &ExampleConfig,
    ) -> Result<ExampleResult, TestError>;

    /// Verify the results are correct
    async fn verify(
        &self,
        env: &dyn TestEnvironment,
        actors: &[ActorHandle],
        result: &ExampleResult,
    ) -> Result<bool, TestError>;

    /// Clean up after the example
    async fn cleanup(
        &self,
        env: &dyn TestEnvironment,
        _actors: Vec<ActorHandle>,
    ) -> Result<(), TestError> {
        env.cleanup().await
    }
}

/// Runner for examples across different environments
pub struct ExampleRunner {
    example: Box<dyn TestableExample>,
    environments: Vec<Box<dyn TestEnvironment>>,
}

impl ExampleRunner {
    pub fn new(example: Box<dyn TestableExample>) -> Self {
        ExampleRunner {
            example,
            environments: Vec::new(),
        }
    }

    pub fn add_environment(mut self, env: Box<dyn TestEnvironment>) -> Self {
        self.environments.push(env);
        self
    }

    /// Run the example in all configured environments
    pub async fn run_all(&self) -> HashMap<String, ExampleResult> {
        let mut results = HashMap::new();

        for env in &self.environments {
            let env_name = format!("{:?}", env.environment_type());
            match self.run_in_environment(&**env).await {
                Ok(result) => {
                    results.insert(env_name, result);
                }
                Err(e) => {
                    results.insert(
                        env_name,
                        ExampleResult {
                            success: false,
                            duration: Duration::from_secs(0),
                            actors_deployed: 0,
                            messages_sent: 0,
                            custom_metrics: HashMap::new(),
                            errors: vec![e.to_string()],
                        },
                    );
                }
            }
        }

        results
    }

    /// Run the example in a specific environment
    pub async fn run_in_environment(
        &self,
        env: &dyn TestEnvironment,
    ) -> Result<ExampleResult, TestError> {
        let config = self.example.config();

        // Deploy actors
        let actors = self.example.deploy_actors(env, &config).await?;

        // Run the example
        let result = self.example.run(env, actors.clone(), &config).await?;

        // Verify results
        let verified = self.example.verify(env, &actors, &result).await?;
        if !verified {
            return Err(TestError::EnvironmentError(
                "Verification failed".to_string(),
            ));
        }

        // Cleanup
        self.example.cleanup(env, actors).await?;

        Ok(result)
    }

    /// Generate comparison report
    pub fn generate_report(&self, results: &HashMap<String, ExampleResult>) {
        println!(
            "\n=== {} - Multi-Environment Results ===",
            self.example.name()
        );
        println!("{}", self.example.description());
        println!();

        println!(
            "{:<15} | {:<10} | {:<12} | {:<12} | {:<10}",
            "Environment", "Success", "Duration", "Actors", "Messages"
        );
        println!("{:-<75}", "");

        for (env, result) in results {
            println!(
                "{:<15} | {:<10} | {:<12.2?} | {:<12} | {:<10}",
                env,
                if result.success { "✅" } else { "❌" },
                result.duration,
                result.actors_deployed,
                result.messages_sent
            );

            if !result.errors.is_empty() {
                for error in &result.errors {
                    println!("                 Error: {}", error);
                }
            }
        }

        println!();
    }
}

/// Macro to simplify example implementation
#[macro_export]
macro_rules! define_example {
    (
        name: $name:expr,
        description: $desc:expr,
        actors: $actors:expr,
        run: $run:expr,
        verify: $verify:expr
    ) => {
        pub struct Example;

        #[async_trait]
        impl TestableExample for Example {
            fn name(&self) -> &str {
                $name
            }

            fn description(&self) -> &str {
                $desc
            }

            fn config(&self) -> ExampleConfig {
                ExampleConfig {
                    name: $name.to_string(),
                    actors: $actors,
                    parameters: HashMap::new(),
                    timeout: Duration::from_secs(30),
                }
            }

            async fn deploy_actors(
                &self,
                env: &dyn TestEnvironment,
                config: &ExampleConfig,
            ) -> Result<Vec<ActorHandle>, TestError> {
                let mut handles = Vec::new();
                for actor_config in &config.actors {
                    handles.push(env.deploy_actor(actor_config.clone()).await?);
                }
                Ok(handles)
            }

            async fn run(
                &self,
                env: &dyn TestEnvironment,
                actors: Vec<ActorHandle>,
                config: &ExampleConfig,
            ) -> Result<ExampleResult, TestError> {
                $run(env, actors, config).await
            }

            async fn verify(
                &self,
                env: &dyn TestEnvironment,
                actors: &[ActorHandle],
                result: &ExampleResult,
            ) -> Result<bool, TestError> {
                $verify(env, actors, result).await
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{in_process::InProcessEnvironment, ResourceProfile};

    struct SimpleExample;

    #[async_trait]
    impl TestableExample for SimpleExample {
        fn name(&self) -> &str {
            "Simple Test"
        }

        fn description(&self) -> &str {
            "A simple test example"
        }

        fn config(&self) -> ExampleConfig {
            ExampleConfig {
                name: "simple".to_string(),
                actors: vec![ActorConfig {
                    id: "actor1".to_string(),
                    actor_type: "simple".to_string(),
                    resources: ResourceProfile::minimal(),
                    metadata: HashMap::new(),
                }],
                parameters: HashMap::new(),
                timeout: Duration::from_secs(5),
            }
        }

        async fn deploy_actors(
            &self,
            env: &dyn TestEnvironment,
            config: &ExampleConfig,
        ) -> Result<Vec<ActorHandle>, TestError> {
            let mut handles = Vec::new();
            for actor_config in &config.actors {
                handles.push(env.deploy_actor(actor_config.clone()).await?);
            }
            Ok(handles)
        }

        async fn run(
            &self,
            _env: &dyn TestEnvironment,
            _actors: Vec<ActorHandle>,
            _config: &ExampleConfig,
        ) -> Result<ExampleResult, TestError> {
            Ok(ExampleResult {
                success: true,
                duration: Duration::from_millis(10),
                actors_deployed: 1,
                messages_sent: 0,
                custom_metrics: HashMap::new(),
                errors: vec![],
            })
        }

        async fn verify(
            &self,
            _env: &dyn TestEnvironment,
            _actors: &[ActorHandle],
            result: &ExampleResult,
        ) -> Result<bool, TestError> {
            Ok(result.success)
        }
    }

    #[tokio::test]
    async fn test_example_runner() {
        let runner = ExampleRunner::new(Box::new(SimpleExample))
            .add_environment(Box::new(InProcessEnvironment::new()));

        let results = runner.run_all().await;
        assert!(!results.is_empty());

        runner.generate_report(&results);
    }
}
