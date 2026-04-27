// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Pure UTF-8 helpers for WIT `payload` (`list<u8>`) at the component boundary.
// Kept separate from actor.ts so Node tests can import without `plexspaces:` virtual imports.

/**
 * Decode WIT `payload` (`list<u8>`) or a UTF-8 string for JSON/text handling in the guest.
 *
 * jco/wasmtime passes `list<u8>` as `Uint8Array`. Using `payload.trim` is wrong (undefined on
 * TypedArray), which skips `JSON.parse` and drops `op` / `message_type` from the body when the
 * host falls back to envelope `msgType` "call".
 *
 * Some embeddings surface other `ArrayBufferView` types; treat any view like `Uint8Array`.
 *
 * @param input - Raw config, message body, or state bytes from the component boundary
 * @returns UTF-8 string (may be empty)
 */
export function decodeWitPayloadUtf8(
  input: string | Uint8Array | ArrayBuffer | ArrayBufferView,
): string {
  if (typeof input === 'string') {
    return input;
  }
  if (input instanceof ArrayBuffer) {
    return new TextDecoder('utf-8', { fatal: false }).decode(new Uint8Array(input));
  }
  if (ArrayBuffer.isView(input)) {
    const v = input as ArrayBufferView;
    return new TextDecoder('utf-8', { fatal: false }).decode(
      new Uint8Array(v.buffer, v.byteOffset, v.byteLength),
    );
  }
  return '';
}

/**
 * Encode a UTF-8 string as WIT `payload` (`list<u8>`) for guest exports.
 *
 * jco maps `list<u8>` to `Uint8Array` on the JavaScript side — not `string`. Returning a plain
 * string from `handle` / `get-state` can lift as an empty success payload at the component ABI.
 *
 * @param text - JSON or other UTF-8 text to pass to the host
 * @returns Bytes for the canonical ABI
 */
export function encodeWitPayloadUtf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}
