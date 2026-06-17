-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Fix: token_hash unique index is not needed for JWT-based tokens.
-- The token_id (PK) is stored in token_hash for lookup; uniqueness is
-- already guaranteed by the primary key.

DROP INDEX IF EXISTS idx_api_tokens_hash;
CREATE INDEX IF NOT EXISTS idx_api_tokens_hash ON api_tokens(token_hash);
