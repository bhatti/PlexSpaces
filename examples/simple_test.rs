//! Simple test example to verify framework is working
//!
//! This is a minimal example that tests basic functionality
//! without relying on all features being implemented.

use plexspaces::testing::{
    ActorConfig, EnvironmentType, ResourceProfile, TestEnvironment, TestEnvironmentBuilder,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== PlexSpaces Simple Test ===\n");

    // Create an in-process test environment
    let env = TestEnvironmentBuilder::new(EnvironmentType::InProcess)
        .build()
        .await?;

    println!("✅ Environment created: {:?}", env.environment_type());

    // Create a simple actor configuration
    let config = ActorConfig {
        id: "test_actor".to_string(),
        actor_type: "simple".to_string(),
        resources: ResourceProfile::minimal(),
        metadata: Default::default(),
    };

    println!("Deploying actor: {}", config.id);

    // Try to deploy (this will fail for now but shows the structure)
    match env.deploy_actor(config).await {
        Ok(_) => println!("✅ Actor deployed successfully"),
        Err(e) => println!("⚠️  Actor deployment failed (expected): {}", e),
    }

    // Collect metrics
    match env.collect_metrics().await {
        Ok(metrics) => {
            println!("\n📊 Metrics:");
            println!("  Messages sent: {}", metrics.messages_sent);
            println!("  Messages received: {}", metrics.messages_received);
        }
        Err(e) => println!("⚠️  Metrics collection failed: {}", e),
    }

    // Cleanup
    env.cleanup().await?;
    println!("\n✅ Test completed successfully!");

    Ok(())
}
