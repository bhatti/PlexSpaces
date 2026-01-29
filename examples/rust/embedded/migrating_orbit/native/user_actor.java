// Orbit User Actor (Java)
// Demonstrates virtual actor lifecycle management

import cloud.orbit.actors.Actor;
import cloud.orbit.concurrent.Task;

public interface UserActor extends Actor {
    Task<Void> updateProfile(String name, String email);
    Task<UserProfile> getProfile();
}

public class UserActorImpl implements UserActor {
    private UserProfile profile = new UserProfile();

    @Override
    public Task<Void> updateProfile(String name, String email) {
        this.profile = new UserProfile(name, email);
        return Task.done();
    }

    @Override
    public Task<UserProfile> getProfile() {
        return Task.fromValue(profile);
    }
}

// Usage:
// UserActor user = Actor.getReference(UserActor.class, "user-123");
// await user.updateProfile("Alice", "alice@example.com");
// UserProfile profile = await user.getProfile();
