// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Manual protobuf wire for plexspaces.wasm.v1.HttpFetchRequest / HttpFetchResponse.
// Used by TinyGo WASM http-fetch (WIT: link-name, method, path-and-query, request: payload).
// Matches prost encoding in crates/wasm-runtime simple_component_host.

package plexspaces

import (
	"encoding/base64"
	"fmt"
	"unicode/utf8"
)

// encodeHttpFetchRequestWire builds HttpFetchRequest: map headers (field 2), body (field 3).
func encodeHttpFetchRequestWire(headers map[string]string, body []byte) ([]byte, error) {
	var buf []byte
	for k, v := range headers {
		if !utf8.ValidString(k) || !utf8.ValidString(v) {
			return nil, fmt.Errorf("http fetch request header key/value must be UTF-8")
		}
		var entry []byte
		entry = appendLengthDelimited(entry, 1, []byte(k))
		entry = appendLengthDelimited(entry, 2, []byte(v))
		buf = appendLengthDelimited(buf, 2, entry)
	}
	if body == nil {
		body = []byte{}
	}
	buf = appendLengthDelimited(buf, 3, body)
	return buf, nil
}

// decodeHttpFetchResponseWire parses HttpFetchResponse into the map shape Host.HTTPFetch returns.
func decodeHttpFetchResponseWire(data []byte) (map[string]any, error) {
	out := map[string]any{
		"status":  float64(0),
		"headers": map[string]any{},
		"body":    "",
	}
	headers := map[string]any{}
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 2 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return nil, err
			}
			pos += m
			out["status"] = float64(v)
		case fn == 3 && wt == 2:
			sl, np, err := readLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			pos = np
			k, v, err := parseStringStringMapEntry(sl)
			if err != nil {
				return nil, err
			}
			headers[k] = v
		case fn == 4 && wt == 2:
			sl, np, err := readLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			pos = np
			if utf8.Valid(sl) {
				out["body"] = string(sl)
			} else {
				out["body"] = base64.StdEncoding.EncodeToString(sl)
			}
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["headers"] = headers
	return out, nil
}

func readLengthDelimited(data []byte, pos int) ([]byte, int, error) {
	ln, n, err := readVarint(data, pos)
	if err != nil {
		return nil, pos, err
	}
	pos += n
	end := pos + int(ln)
	if end > len(data) {
		return nil, pos, fmt.Errorf("length-delimited field truncated")
	}
	return data[pos:end], end, nil
}

func parseStringStringMapEntry(entry []byte) (key, val string, err error) {
	pos := 0
	for pos < len(entry) {
		tag, n, err := readVarint(entry, pos)
		if err != nil {
			return "", "", err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := readLengthDelimited(entry, pos)
			if err != nil {
				return "", "", err
			}
			pos = np
			key = string(sl)
		case fn == 2 && wt == 2:
			sl, np, err := readLengthDelimited(entry, pos)
			if err != nil {
				return "", "", err
			}
			pos = np
			val = string(sl)
		default:
			pos, err = skipField(entry, pos, wt)
			if err != nil {
				return "", "", err
			}
		}
	}
	return key, val, nil
}
