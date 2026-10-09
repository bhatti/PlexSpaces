// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// SafeMarshal — TinyGo WASM-safe JSON encoding.
//
// TinyGo WASM has two known json.Marshal crashes:
//   1. float64 in map[string]any → strconv.genericFtoa → WASM trap
//   2. nested map[string]any → fmtsort.Sort → slices.SortStableFunc[fmtsort.KeyValue]
//      dispatches to the wrong function-table entry (base64.decodeQuantum) → WASM trap
//
// SafeMarshal avoids both by using integer arithmetic for float64 and unsorted
// map iteration (JSON spec allows any key order). Actor code should call
// SafeMarshal instead of json.Marshal for response maps.

//go:build !wasm

package plexspaces

import (
	"encoding/json"
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
		// Fallback for structs and other types. Callers should avoid putting
		// struct values with float64 fields into SafeMarshal maps on TinyGo WASM.
		b, _ := json.Marshal(v)
		sb.Write(b)
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
