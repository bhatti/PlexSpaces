# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Storefront API – Real-world e-commerce service on host KV

A single WASM actor implementing a **storefront backend**: store configuration,
shopping carts (sessions), and checkout rate limiting. All state is in the
framework's host keyvalue store (Redis/SQLite), so it is durable, shared across
instances, and survives restarts.

Use cases:
- **Store config**: Free-shipping threshold, tax rate, currency, feature flags.
- **Shopping cart**: Create/get/update/destroy cart per session; list carts by user.
- **Checkout rate limit**: Throttle checkout attempts per user or API key (e.g. 5/min).

Keys are namespaced (config:, cart:, ratelimit:) so one actor serves the full API.
"""

import json
import time
from plexspaces import actor, handler, init_handler, host


PREFIX_CONFIG = "config:"
PREFIX_CART = "cart:"
PREFIX_RATELIMIT = "ratelimit:"


def _kv_get(key: str) -> str:
    r = host.kv.get(key)
    return r if r and not r.startswith("ERROR") else ""


def _kv_put(key: str, value: str) -> bool:
    r = host.kv.put(key, value)
    return not (r and r.startswith("ERROR"))


def _kv_delete(key: str) -> bool:
    r = host.kv.delete(key)
    return not (r and r.startswith("ERROR"))


def _kv_list(prefix: str) -> list:
    r = host.kv.list(prefix)
    if not r or r.startswith("ERROR"):
        return []
    try:
        return json.loads(r)
    except json.JSONDecodeError:
        return []


@actor
class StorefrontService:
    """
    Storefront API: store config, shopping carts, and checkout rate limiting
    backed by the framework's host KV.
    """

    @init_handler
    def on_init(self, config: dict) -> None:
        """Initialize storefront; optional default config can be loaded here."""
        host.log("info", "StorefrontService initialized (config + cart + checkout rate limit)")

    # ---------- Store configuration ----------

    @handler("set_store_config")
    def set_store_config(self, key: str = "", value: str = "") -> dict:
        """Set a store config entry (e.g. free_shipping_threshold, tax_rate, currency)."""
        if not key:
            return {"error": "key required"}
        k = f"{PREFIX_CONFIG}{key}"
        if not _kv_put(k, value):
            return {"error": "config set failed"}
        return {"status": "ok", "key": key}

    @handler("get_store_config")
    def get_store_config(self, key: str = "") -> dict:
        """Get a store config value."""
        if not key:
            return {"error": "key required"}
        value = _kv_get(f"{PREFIX_CONFIG}{key}")
        if value == "":
            return {"found": False, "key": key}
        return {"found": True, "key": key, "value": value}

    @handler("list_store_config")
    def list_store_config(self, prefix: str = "") -> dict:
        """List store config keys (optional prefix filter)."""
        keys = _kv_list(PREFIX_CONFIG + prefix)
        short = [k[len(PREFIX_CONFIG):] for k in keys]
        return {"keys": short, "count": len(short)}

    # ---------- Shopping cart (session store) ----------

    @handler("create_cart")
    def create_cart(
        self,
        cart_id: str = "",
        user_id: str = "",
        items: str = "[]",
    ) -> dict:
        """Create or overwrite a cart. items is JSON array of {sku, qty, price}."""
        if not cart_id:
            return {"error": "cart_id required"}
        k = f"{PREFIX_CART}{cart_id}"
        payload = {
            "user_id": user_id,
            "created_at": int(time.time()),
            "updated_at": int(time.time()),
            "items": items,
        }
        if not _kv_put(k, json.dumps(payload)):
            return {"error": "cart create failed"}
        return {"status": "ok", "cart_id": cart_id, "user_id": user_id}

    @handler("get_cart")
    def get_cart(self, cart_id: str = "") -> dict:
        """Get a cart by id."""
        if not cart_id:
            return {"error": "cart_id required"}
        raw = _kv_get(f"{PREFIX_CART}{cart_id}")
        if not raw:
            return {"found": False, "cart_id": cart_id}
        try:
            data = json.loads(raw)
            return {"found": True, "cart_id": cart_id, "cart": data}
        except json.JSONDecodeError:
            return {"found": True, "cart_id": cart_id, "raw": raw}

    @handler("update_cart")
    def update_cart(self, cart_id: str = "", items: str = "[]") -> dict:
        """Update cart items. items is JSON array of {sku, qty, price}."""
        if not cart_id:
            return {"error": "cart_id required"}
        k = f"{PREFIX_CART}{cart_id}"
        raw = _kv_get(k)
        if not raw:
            return {"error": "cart not found", "cart_id": cart_id}
        try:
            data = json.loads(raw)
            data["items"] = items
            data["updated_at"] = int(time.time())
            if not _kv_put(k, json.dumps(data)):
                return {"error": "cart update failed"}
            return {"status": "ok", "cart_id": cart_id}
        except (json.JSONDecodeError, TypeError):
            return {"error": "invalid cart data"}

    @handler("destroy_cart")
    def destroy_cart(self, cart_id: str = "") -> dict:
        """Destroy a cart (e.g. after checkout)."""
        if not cart_id:
            return {"error": "cart_id required"}
        ok = _kv_delete(f"{PREFIX_CART}{cart_id}")
        return {"status": "ok" if ok else "error", "cart_id": cart_id}

    @handler("list_carts")
    def list_carts(self, prefix: str = "") -> dict:
        """List cart ids (optional prefix, e.g. user_id)."""
        full_prefix = PREFIX_CART + prefix
        keys = _kv_list(full_prefix)
        ids = [k[len(PREFIX_CART):] for k in keys]
        return {"cart_ids": ids, "count": len(ids)}

    # ---------- Checkout rate limit ----------

    @handler("checkout_allowed")
    def checkout_allowed(
        self,
        identity: str = "",
        window_sec: int = 60,
        max_requests: int = 5,
    ) -> dict:
        """
        Check and consume one checkout attempt for identity in the current window.
        Returns allowed, remaining, and reset time (e.g. 5 checkouts per minute per user).
        """
        if not identity:
            return {"error": "identity required"}
        if window_sec < 1:
            window_sec = 60
        if max_requests < 1:
            max_requests = 5

        now = int(time.time())
        bucket = now // window_sec
        key = f"{PREFIX_RATELIMIT}checkout:{identity}:{bucket}"

        raw = _kv_get(key)
        try:
            count = int(raw) if raw else 0
        except ValueError:
            count = 0

        if count >= max_requests:
            return {
                "allowed": False,
                "remaining": 0,
                "limit": max_requests,
                "reset_at": (bucket + 1) * window_sec,
                "identity": identity,
            }

        count += 1
        _kv_put(key, str(count))

        return {
            "allowed": True,
            "remaining": max_requests - count,
            "limit": max_requests,
            "reset_at": (bucket + 1) * window_sec,
            "identity": identity,
        }
