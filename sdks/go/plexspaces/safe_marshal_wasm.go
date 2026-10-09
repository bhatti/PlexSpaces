// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// SafeMarshal — TinyGo WASM-safe JSON encoding (WASM build).
//
// This file is the WASM variant of safe_marshal.go. It deliberately does NOT import
// encoding/json. In TinyGo 0.40.x, importing encoding/json (even indirectly) links
// fmtsort's comparison function into the WASM function table at a slot that collides
// with actor interface method dispatch, causing Handle$invoke to call the wrong
// function. Removing all json.Marshal calls from WASM-compiled code paths lets
// TinyGo's LLVM DCE eliminate fmtsort entirely.
//
// The only difference from safe_marshal.go: the default case emits "null" instead
// of calling json.Marshal(v). All explicitly-typed cases (int, float64, string,
// map[string]any, []int64, etc.) are identical.

//go:build wasm

package plexspaces

import (
	"math"
	"strconv"
	"strings"
)

// SafeMarshal encodes v as a JSON string, working around TinyGo WASM bugs in
// encoding/json. Safe for use in map[string]any response values that contain
// float64 or nested map[string]any.
//
// Use in actor code instead of json.Marshal for actor response maps:
//
//	func marshal(v any) string { return plexspaces.SafeMarshal(v) }
func SafeMarshal(v any) string {
	var sb strings.Builder
	safeMarshalInto(&sb, v)
	return sb.String()
}

func safeMarshalInto(sb *strings.Builder, v any) {
	if v == nil {
		sb.WriteString("null")
		return
	}
	switch val := v.(type) {
	case bool:
		if val {
			sb.WriteString("true")
		} else {
			sb.WriteString("false")
		}
	case int:
		sb.WriteString(strconv.FormatInt(int64(val), 10))
	case int32:
		sb.WriteString(strconv.FormatInt(int64(val), 10))
	case int64:
		sb.WriteString(strconv.FormatInt(val, 10))
	case uint:
		sb.WriteString(strconv.FormatUint(uint64(val), 10))
	case uint32:
		sb.WriteString(strconv.FormatUint(uint64(val), 10))
	case uint64:
		sb.WriteString(strconv.FormatUint(val, 10))
	case float32:
		safeWriteFloat(sb, float64(val))
	case float64:
		safeWriteFloat(sb, val)
	case string:
		writeJSONString(sb, val)
	case map[string]any:
		safeMarshalMap(sb, val)
	case []any:
		safeMarshalAnySlice(sb, val)
	case []map[string]any:
		sb.WriteByte('[')
		for i, m := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			safeMarshalMap(sb, m)
		}
		sb.WriteByte(']')
	case []string:
		sb.WriteByte('[')
		for i, s := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			writeJSONString(sb, s)
		}
		sb.WriteByte(']')
	case []int:
		sb.WriteByte('[')
		for i, n := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			sb.WriteString(strconv.FormatInt(int64(n), 10))
		}
		sb.WriteByte(']')
	case []int64:
		sb.WriteByte('[')
		for i, n := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			sb.WriteString(strconv.FormatInt(n, 10))
		}
		sb.WriteByte(']')
	case [][]int64:
		sb.WriteByte('[')
		for i, row := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			sb.WriteByte('[')
			for j, n := range row {
				if j > 0 {
					sb.WriteByte(',')
				}
				sb.WriteString(strconv.FormatInt(n, 10))
			}
			sb.WriteByte(']')
		}
		sb.WriteByte(']')
	case []float64:
		sb.WriteByte('[')
		for i, f := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			safeWriteFloat(sb, f)
		}
		sb.WriteByte(']')
	case [][]float64:
		sb.WriteByte('[')
		for i, row := range val {
			if i > 0 {
				sb.WriteByte(',')
			}
			sb.WriteByte('[')
			for j, f := range row {
				if j > 0 {
					sb.WriteByte(',')
				}
				safeWriteFloat(sb, f)
			}
			sb.WriteByte(']')
		}
		sb.WriteByte(']')
	case map[string]string:
		sb.WriteByte('{')
		first := true
		for k, s := range val {
			if !first {
				sb.WriteByte(',')
			}
			first = false
			writeJSONString(sb, k)
			sb.WriteByte(':')
			writeJSONString(sb, s)
		}
		sb.WriteByte('}')
	default:
		// TinyGo WASM: encoding/json.Marshal must not be called — it links fmtsort into
		// the WASM function table, corrupting interface dispatch. Return null for unknown
		// types. Actors should use explicitly-typed fields instead of opaque structs.
		_ = val
		sb.WriteString("null")
	}
}

func safeMarshalMap(sb *strings.Builder, m map[string]any) {
	sb.WriteByte('{')
	first := true
	for k, v := range m { // unsorted — valid JSON, avoids fmtsort.Sort crash
		if !first {
			sb.WriteByte(',')
		}
		first = false
		writeJSONString(sb, k)
		sb.WriteByte(':')
		safeMarshalInto(sb, v)
	}
	sb.WriteByte('}')
}

func safeMarshalAnySlice(sb *strings.Builder, s []any) {
	sb.WriteByte('[')
	for i, v := range s {
		if i > 0 {
			sb.WriteByte(',')
		}
		safeMarshalInto(sb, v)
	}
	sb.WriteByte(']')
}

// safeWriteFloat writes a float64 as a JSON number without calling strconv.AppendFloat,
// which triggers a WASM trap in TinyGo. Uses integer arithmetic to build the decimal
// representation with up to 3 decimal places of precision.
func safeWriteFloat(sb *strings.Builder, f float64) {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		sb.WriteByte('0')
		return
	}
	if f < 0 {
		sb.WriteByte('-')
		f = -f
	}
	// Scale to 3 decimal places using integer arithmetic only.
	const scale = 1000
	scaled := int64(f*scale + 0.5)
	intPart := scaled / scale
	fracPart := scaled % scale
	sb.WriteString(strconv.FormatInt(intPart, 10))
	if fracPart > 0 {
		sb.WriteByte('.')
		fstr := strconv.FormatInt(fracPart, 10)
		for len(fstr) < 3 {
			fstr = "0" + fstr
		}
		for len(fstr) > 1 && fstr[len(fstr)-1] == '0' {
			fstr = fstr[:len(fstr)-1]
		}
		sb.WriteString(fstr)
	}
}

// ---- WASM-safe JSON read helpers ----
// These avoid encoding/json entirely to prevent TinyGo WASM function-table corruption.

// WasmJSONConsumeParsedString parses a JSON string starting with '"' at s[0].
// Returns (value, remaining_input). Handles common escapes (\n \r \t \\ \").
func WasmJSONConsumeParsedString(s string) (string, string) {
	if len(s) == 0 || s[0] != '"' {
		return "", s
	}
	var sb strings.Builder
	i := 1
	for i < len(s) {
		c := s[i]
		if c == '"' {
			i++
			break
		}
		if c == '\\' && i+1 < len(s) {
			i++
			switch s[i] {
			case '"':
				sb.WriteByte('"')
			case '\\':
				sb.WriteByte('\\')
			case '/':
				sb.WriteByte('/')
			case 'n':
				sb.WriteByte('\n')
			case 'r':
				sb.WriteByte('\r')
			case 't':
				sb.WriteByte('\t')
			default:
				sb.WriteByte(s[i])
			}
		} else {
			sb.WriteByte(c)
		}
		i++
	}
	return sb.String(), s[i:]
}

// WasmJSONBracketedContent returns s up to and including the matching close bracket.
// s must start with open ('{' or '['). Returns "" on unmatched input.
func WasmJSONBracketedContent(s string, open, close byte) string {
	if len(s) == 0 || s[0] != open {
		return ""
	}
	depth := 0
	inStr := false
	for i := 0; i < len(s); i++ {
		c := s[i]
		if inStr {
			if c == '\\' {
				i++
			} else if c == '"' {
				inStr = false
			}
			continue
		}
		switch c {
		case '"':
			inStr = true
		default:
			if c == open {
				depth++
			} else if c == close {
				depth--
				if depth == 0 {
					return s[:i+1]
				}
			}
		}
	}
	return ""
}

// wasmFindValueAt returns the index in obj immediately after "key": (with any
// trailing whitespace skipped). Zero allocations: byte-level scan, no string
// concatenation, no closure creation. Returns -1 if the key is not present.
func wasmFindValueAt(obj, key string) int {
	klen := len(key)
	for i := 0; i+klen+2 < len(obj); i++ {
		if obj[i] != '"' {
			continue
		}
		end := i + 1 + klen
		if end >= len(obj) {
			break
		}
		// fast byte-level key match
		match := true
		for j := 0; j < klen; j++ {
			if obj[i+1+j] != key[j] {
				match = false
				break
			}
		}
		if !match {
			continue
		}
		// must be followed by closing quote then colon
		colon := end + 1
		if obj[end] != '"' || colon >= len(obj) || obj[colon] != ':' {
			continue
		}
		pos := colon + 1
		for pos < len(obj) && (obj[pos] == ' ' || obj[pos] == '\t' || obj[pos] == '\n' || obj[pos] == '\r') {
			pos++
		}
		return pos
	}
	return -1
}

// wasmParseInt64At parses a signed decimal integer starting at obj[pos].
// Zero allocations: no strconv, no string slicing.
func wasmParseInt64At(obj string, pos int) int64 {
	if pos >= len(obj) {
		return 0
	}
	sign := int64(1)
	if obj[pos] == '-' {
		sign = -1
		pos++
	}
	if pos >= len(obj) || obj[pos] < '0' || obj[pos] > '9' {
		return 0
	}
	var n int64
	for pos < len(obj) && obj[pos] >= '0' && obj[pos] <= '9' {
		n = n*10 + int64(obj[pos]-'0')
		pos++
	}
	return sign * n
}

// wasmParseUint64At parses an unsigned decimal integer starting at obj[pos].
// Zero allocations.
func wasmParseUint64At(obj string, pos int) uint64 {
	var n uint64
	for pos < len(obj) && obj[pos] >= '0' && obj[pos] <= '9' {
		n = n*10 + uint64(obj[pos]-'0')
		pos++
	}
	return n
}

// WasmJSONFindObject returns the "{...}" for "key":{...} in obj.
// Zero allocations.
func WasmJSONFindObject(obj, key string) string {
	pos := wasmFindValueAt(obj, key)
	if pos < 0 {
		return ""
	}
	return WasmJSONBracketedContent(obj[pos:], '{', '}')
}

// WasmJSONFindArray returns the "[...]" for "key":[...] in obj.
// Zero allocations.
func WasmJSONFindArray(obj, key string) string {
	pos := wasmFindValueAt(obj, key)
	if pos < 0 {
		return ""
	}
	return WasmJSONBracketedContent(obj[pos:], '[', ']')
}

// WasmJSONExtractStr returns the string value for "key":"value" in obj.
func WasmJSONExtractStr(obj, key string) string {
	pos := wasmFindValueAt(obj, key)
	if pos < 0 {
		return ""
	}
	v, _ := WasmJSONConsumeParsedString(obj[pos:])
	return v
}

// WasmJSONExtractInt64 returns the int64 value for "key":N in obj.
// Zero allocations.
func WasmJSONExtractInt64(obj, key string) int64 {
	pos := wasmFindValueAt(obj, key)
	if pos < 0 {
		return 0
	}
	return wasmParseInt64At(obj, pos)
}

// WasmJSONExtractUint64 returns the uint64 value for "key":N in obj.
// Zero allocations.
func WasmJSONExtractUint64(obj, key string) uint64 {
	pos := wasmFindValueAt(obj, key)
	if pos < 0 {
		return 0
	}
	return wasmParseUint64At(obj, pos)
}

// WasmJSONExtractInt returns the int value for "key":N in obj.
// Zero allocations.
func WasmJSONExtractInt(obj, key string) int {
	return int(WasmJSONExtractInt64(obj, key))
}

// WasmJSONExtractNestedStr returns obj.key1.key2 as a string.
func WasmJSONExtractNestedStr(obj, key1, key2 string) string {
	inner := WasmJSONFindObject(obj, key1)
	if inner == "" {
		return ""
	}
	return WasmJSONExtractStr(inner, key2)
}

// WasmJSONSplitArray splits the interior of a JSON array by top-level commas.
func WasmJSONSplitArray(inner string) []string {
	var parts []string
	depth := 0
	inStr := false
	start := 0
	for i := 0; i < len(inner); i++ {
		c := inner[i]
		if inStr {
			if c == '\\' {
				i++
			} else if c == '"' {
				inStr = false
			}
			continue
		}
		switch c {
		case '"':
			inStr = true
		case '[', '{':
			depth++
		case ']', '}':
			depth--
		case ',':
			if depth == 0 {
				parts = append(parts, inner[start:i])
				start = i + 1
			}
		}
	}
	parts = append(parts, inner[start:])
	return parts
}

// WasmJSONParseInt64ArrayInto parses "[N,N,N,...]" directly into dst without any allocations.
// Returns the number of elements written. Safe to call without triggering GC.
//
//go:noinline
func WasmJSONParseInt64ArrayInto(arrJSON string, dst []int64) int {
	if len(arrJSON) < 2 || arrJSON[0] != '[' {
		return 0
	}
	idx := 0
	i := 1
	for i < len(arrJSON) && idx < len(dst) {
		for i < len(arrJSON) && (arrJSON[i] == ' ' || arrJSON[i] == '\t' || arrJSON[i] == '\n' || arrJSON[i] == '\r') {
			i++
		}
		if i >= len(arrJSON) || arrJSON[i] == ']' {
			break
		}
		sign := int64(1)
		if arrJSON[i] == '-' {
			sign = -1
			i++
		}
		if i >= len(arrJSON) || arrJSON[i] < '0' || arrJSON[i] > '9' {
			for i < len(arrJSON) && arrJSON[i] != ',' && arrJSON[i] != ']' {
				i++
			}
			if i < len(arrJSON) && arrJSON[i] == ',' {
				i++
			}
			continue
		}
		var n int64
		for i < len(arrJSON) && arrJSON[i] >= '0' && arrJSON[i] <= '9' {
			n = n*10 + int64(arrJSON[i]-'0')
			i++
		}
		dst[idx] = sign * n
		idx++
		for i < len(arrJSON) && arrJSON[i] != ',' && arrJSON[i] != ']' {
			i++
		}
		if i < len(arrJSON) && arrJSON[i] == ',' {
			i++
		}
	}
	return idx
}

// WasmJSONParseInt64MatrixInto parses "[[N,...],[N,...],...]" into dst without any allocations.
// dst must be pre-allocated with the expected dimensions. Returns the number of rows written.
//
//go:noinline
func WasmJSONParseInt64MatrixInto(matJSON string, dst [][]int64) int {
	if len(matJSON) < 2 || matJSON[0] != '[' {
		return 0
	}
	rowIdx := 0
	i := 1
	for i < len(matJSON) && rowIdx < len(dst) {
		for i < len(matJSON) && (matJSON[i] == ',' || matJSON[i] == ' ' || matJSON[i] == '\t' || matJSON[i] == '\n' || matJSON[i] == '\r') {
			i++
		}
		if i >= len(matJSON) || matJSON[i] == ']' {
			break
		}
		if matJSON[i] != '[' {
			i++
			continue
		}
		rowStart := i
		depth := 0
		for i < len(matJSON) {
			if matJSON[i] == '[' {
				depth++
			} else if matJSON[i] == ']' {
				depth--
				if depth == 0 {
					i++
					break
				}
			}
			i++
		}
		if depth == 0 {
			WasmJSONParseInt64ArrayInto(matJSON[rowStart:i], dst[rowIdx])
			rowIdx++
		}
	}
	return rowIdx
}

// WasmJSONParseInt64Array parses "[N,N,N,...]" into []int64.
func WasmJSONParseInt64Array(arrJSON string) []int64 {
	if len(arrJSON) < 2 || arrJSON[0] != '[' {
		return nil
	}
	inner := arrJSON[1 : len(arrJSON)-1]
	if strings.TrimSpace(inner) == "" {
		return []int64{}
	}
	parts := WasmJSONSplitArray(inner)
	result := make([]int64, 0, len(parts))
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if p == "" {
			continue
		}
		n, err := strconv.ParseInt(p, 10, 64)
		if err != nil {
			continue
		}
		result = append(result, n)
	}
	return result
}

// WasmJSONParseInt64Matrix parses "[[N,...],[N,...],...]" into [][]int64.
func WasmJSONParseInt64Matrix(matJSON string) [][]int64 {
	if len(matJSON) < 2 || matJSON[0] != '[' {
		return nil
	}
	inner := matJSON[1 : len(matJSON)-1]
	if strings.TrimSpace(inner) == "" {
		return [][]int64{}
	}
	rows := WasmJSONSplitArray(inner)
	result := make([][]int64, 0, len(rows))
	for _, row := range rows {
		row = strings.TrimSpace(row)
		if len(row) > 0 && row[0] == '[' {
			result = append(result, WasmJSONParseInt64Array(row))
		}
	}
	return result
}

// WasmJSONExtractStringStringMap parses {"k":"v",...} into map[string]string.
func WasmJSONExtractStringStringMap(obj string) map[string]string {
	result := map[string]string{}
	if len(obj) < 2 || obj[0] != '{' {
		return result
	}
	s := obj[1:]
	for {
		s = strings.TrimLeft(s, " \t\n\r,}")
		if len(s) == 0 || s[0] == '}' {
			break
		}
		if s[0] != '"' {
			break
		}
		key, rest := WasmJSONConsumeParsedString(s)
		rest = strings.TrimLeft(rest, " \t\n\r")
		if len(rest) == 0 || rest[0] != ':' {
			break
		}
		rest = strings.TrimLeft(rest[1:], " \t\n\r")
		if len(rest) == 0 || rest[0] != '"' {
			break
		}
		val, rest2 := WasmJSONConsumeParsedString(rest)
		result[key] = val
		s = rest2
	}
	return result
}

func writeJSONString(sb *strings.Builder, s string) {
	sb.WriteByte('"')
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch c {
		case '"':
			sb.WriteString(`\"`)
		case '\\':
			sb.WriteString(`\\`)
		case '\n':
			sb.WriteString(`\n`)
		case '\r':
			sb.WriteString(`\r`)
		case '\t':
			sb.WriteString(`\t`)
		default:
			if c < 0x20 {
				sb.WriteString(`\u00`)
				sb.WriteByte("0123456789abcdef"[c>>4])
				sb.WriteByte("0123456789abcdef"[c&0xf])
			} else {
				sb.WriteByte(c)
			}
		}
	}
	sb.WriteByte('"')
}
