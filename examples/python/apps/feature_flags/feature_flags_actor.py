"""
Feature Flags Service - Python WASM Actor (with SDK)

A feature flag management service for controlling feature rollouts.
Real-world use case: A/B testing, gradual rollouts, kill switches.

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent flags storage
- @handler(): Routes flag operations
"""

from plexspaces import actor, state, handler, init_handler


@actor
class FeatureFlagsService:
    """Feature flags actor for A/B testing and rollouts."""
    
    # Feature flags storage: {flag_name: {enabled, rollout}}
    flags: dict = state(default_factory=dict)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize with optional preset flags."""
        self.flags = config.get("flags", {})
    
    @handler("create")
    def create_flag(self, flag: str = "") -> dict:
        """Create a new feature flag."""
        if not flag:
            return {"error": "flag_required"}
        if flag in self.flags:
            return {"error": "flag_exists"}
        self.flags[flag] = {"enabled": False, "rollout": 100}
        return {"status": "ok"}
    
    @handler("enable")
    def enable_flag(self, flag: str = "") -> dict:
        """Enable a feature flag."""
        if flag not in self.flags:
            return {"error": "flag_not_found"}
        self.flags[flag]["enabled"] = True
        return {"status": "ok"}
    
    @handler("disable")
    def disable_flag(self, flag: str = "") -> dict:
        """Disable a feature flag."""
        if flag not in self.flags:
            return {"error": "flag_not_found"}
        self.flags[flag]["enabled"] = False
        return {"status": "ok"}
    
    @handler("rollout")
    def set_rollout(self, flag: str = "", pct: int = 100) -> dict:
        """Set rollout percentage for a flag."""
        if flag not in self.flags:
            return {"error": "flag_not_found"}
        self.flags[flag]["rollout"] = pct
        return {"status": "ok"}
    
    @handler("check")
    def check_flag(self, flag: str = "", user: str = "") -> dict:
        """Check if flag is enabled for user."""
        if not flag:
            return {"error": "flag_required"}
        if flag not in self.flags:
            return {"enabled": False, "reason": "not_found"}
        
        f = self.flags[flag]
        if not f["enabled"]:
            return {"enabled": False, "reason": "disabled"}
        
        rollout = f["rollout"]
        if rollout >= 100:
            return {"enabled": True, "reason": "full"}
        
        # Simple deterministic hash for rollout
        h = 0
        for c in (flag + user):
            h = (h + ord(c)) % 100
        enabled = h < rollout
        return {"enabled": enabled}
    
    @handler("list")
    def list_flags(self) -> dict:
        """List all flags."""
        return {"status": "ok", "count": len(self.flags)}
    
    @handler("delete")
    def delete_flag(self, flag: str = "") -> dict:
        """Delete a feature flag."""
        if flag not in self.flags:
            return {"error": "flag_not_found"}
        del self.flags[flag]
        return {"status": "ok"}
