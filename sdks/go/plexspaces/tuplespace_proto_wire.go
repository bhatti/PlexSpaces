// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Protobuf wire encoding/decoding for plexspaces.tuplespace.v1 WriteRequest, ReadRequest,
// and ReadResponse (subset used by WASM host imports). Matches prost output used in
// crates/wasm-runtime simple_component_host and examples/rust/apps/abstractions.

package plexspaces

import (
	"fmt"
	"math"
	"strings"
	"unicode/utf8"
)

func appendVarint(buf []byte, x uint64) []byte {
	for x >= 0x80 {
		buf = append(buf, byte(x)|0x80)
		x >>= 7
	}
	return append(buf, byte(x))
}

func appendFixed64LE(buf []byte, v uint64) []byte {
	for i := 0; i < 8; i++ {
		buf = append(buf, byte(v))
		v >>= 8
	}
	return buf
}

func appendLengthDelimited(buf []byte, fieldNum int, inner []byte) []byte {
	tag := uint64(fieldNum<<3 | 2)
	buf = appendVarint(buf, tag)
	buf = appendVarint(buf, uint64(len(inner)))
	return append(buf, inner...)
}

func encodeTupleField(v any, allowWildcardStar bool) ([]byte, error) {
	var inner []byte
	switch x := v.(type) {
	case nil:
		inner = appendVarint(append([]byte{}, 0x38), 1) // wildcard, field 7
	case string:
		if allowWildcardStar && x == "*" {
			inner = appendVarint(append([]byte{}, 0x38), 1)
			return inner, nil
		}
		if !utf8.ValidString(x) {
			return nil, fmt.Errorf("tuple field string must be valid UTF-8")
		}
		inner = append(inner, 0x1a) // field 3 string
		inner = appendVarint(inner, uint64(len(x)))
		inner = append(inner, x...)
	case bool:
		inner = append(inner, 0x20) // field 4 boolean
		if x {
			inner = appendVarint(inner, 1)
		} else {
			inner = appendVarint(inner, 0)
		}
	case float64:
		if x == math.Trunc(x) && x >= float64(math.MinInt64) && x <= float64(math.MaxInt64) {
			inner = appendVarint(append([]byte{}, 0x08), uint64(int64(x))) // field 1 int64
		} else {
			inner = append(inner, 0x11) // field 2 double
			inner = appendFixed64LE(inner, math.Float64bits(x))
		}
	default:
		return nil, fmt.Errorf("unsupported tuple field type %T", v)
	}
	return inner, nil
}

func encodeTupleFields(tuple []any, allowWildcardStar bool) ([]byte, error) {
	var out []byte
	for _, el := range tuple {
		tf, err := encodeTupleField(el, allowWildcardStar)
		if err != nil {
			return nil, err
		}
		out = appendLengthDelimited(out, 2, tf)
	}
	return out, nil
}

func encodeWriteRequest(tuple []any) ([]byte, error) {
	tupleBody, err := encodeTupleFields(tuple, false)
	if err != nil {
		return nil, err
	}
	return appendLengthDelimited(nil, 2, tupleBody), nil
}

func encodeReadRequest(pattern []any, take bool, maxResults int32) ([]byte, error) {
	// templateBody is a complete Tuple message (repeated TupleField fields = 2 only).
	templateBody, err := encodeTupleFields(pattern, true)
	if err != nil {
		return nil, err
	}
	var out []byte
	out = appendLengthDelimited(out, 2, templateBody)
	if take {
		out = append(out, 0x28) // field 5 take = true
		out = appendVarint(out, 1)
	}
	out = append(out, 0x30) // field 6 max_results
	out = appendVarint(out, uint64(uint32(maxResults)))
	return out, nil
}

func readVarint(data []byte, pos int) (uint64, int, error) {
	var x uint64
	var s uint
	orig := pos
	for i := 0; i < 10; i++ {
		if pos >= len(data) {
			return 0, 0, fmt.Errorf("varint buffer underflow")
		}
		b := data[pos]
		pos++
		if b < 0x80 {
			return x | uint64(b)<<s, pos - orig, nil
		}
		x |= uint64(b&0x7f) << s
		s += 7
	}
	return 0, 0, fmt.Errorf("varint too long")
}

func skipField(data []byte, pos int, wireType uint64) (int, error) {
	switch wireType {
	case 0:
		_, n, err := readVarint(data, pos)
		return pos + n, err
	case 1:
		if pos+8 > len(data) {
			return 0, fmt.Errorf("fixed64 underflow")
		}
		return pos + 8, nil
	case 2:
		ln, n, err := readVarint(data, pos)
		if err != nil {
			return 0, err
		}
		end := pos + n + int(ln)
		if end > len(data) {
			return 0, fmt.Errorf("length-delimited underflow")
		}
		return end, nil
	case 5:
		if pos+4 > len(data) {
			return 0, fmt.Errorf("fixed32 underflow")
		}
		return pos + 4, nil
	default:
		return 0, fmt.Errorf("unknown wire type %d", wireType)
	}
}

func parseTupleFieldMsg(msg []byte) (any, error) {
	pos := 0
	var last any
	for pos < len(msg) {
		tag, n, err := readVarint(msg, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		switch wt {
		case 0:
			v, m, err := readVarint(msg, pos)
			if err != nil {
				return nil, err
			}
			pos += m
			switch fn {
			case 1:
				last = int64(v)
			case 4:
				last = v != 0
			case 6, 7:
				last = nil
			default:
			}
		case 1:
			if pos+8 > len(msg) {
				return nil, fmt.Errorf("double underflow")
			}
			u := uint64(msg[pos]) | uint64(msg[pos+1])<<8 | uint64(msg[pos+2])<<16 | uint64(msg[pos+3])<<24 |
				uint64(msg[pos+4])<<32 | uint64(msg[pos+5])<<40 | uint64(msg[pos+6])<<48 | uint64(msg[pos+7])<<56
			pos += 8
			if fn == 2 {
				last = math.Float64frombits(u)
			}
		case 2:
			ln, m, err := readVarint(msg, pos)
			if err != nil {
				return nil, err
			}
			pos += m
			end := pos + int(ln)
			if end > len(msg) {
				return nil, fmt.Errorf("bytes underflow")
			}
			chunk := msg[pos:end]
			pos = end
			if fn == 3 || fn == 5 {
				last = string(chunk)
			}
		default:
			pos, err = skipField(msg, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	if last == nil {
		return nil, nil
	}
	return last, nil
}

func parseTupleMsg(msg []byte) ([]any, error) {
	pos := 0
	var fields []any
	for pos < len(msg) {
		tag, n, err := readVarint(msg, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 2 && wt == 2 {
			ln, m, err := readVarint(msg, pos)
			if err != nil {
				return nil, err
			}
			pos += m
			end := pos + int(ln)
			if end > len(msg) {
				return nil, fmt.Errorf("tuple field underflow")
			}
			sub := msg[pos:end]
			pos = end
			fv, err := parseTupleFieldMsg(sub)
			if err != nil {
				return nil, err
			}
			fields = append(fields, fv)
		} else {
			pos, err = skipField(msg, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	return fields, nil
}

func parseReadResponseTuples(data []byte) ([][]any, error) {
	pos := 0
	var tuples [][]any
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 2 && wt == 2 {
			ln, m, err := readVarint(data, pos)
			if err != nil {
				return nil, err
			}
			pos += m
			end := pos + int(ln)
			if end > len(data) {
				return nil, fmt.Errorf("tuple message underflow")
			}
			tup, err := parseTupleMsg(data[pos:end])
			if err != nil {
				return nil, err
			}
			pos = end
			tuples = append(tuples, tup)
		} else {
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	return tuples, nil
}

func decodeReadResponseFirstTuple(raw string) ([]any, bool) {
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil, false
	}
	tuples, err := parseReadResponseTuples([]byte(raw))
	if err != nil || len(tuples) == 0 {
		return nil, false
	}
	return tuples[0], true
}

func decodeReadResponseAllTuples(raw string) [][]any {
	if strings.HasPrefix(raw, errorPrefix) {
		return nil
	}
	if raw == "" {
		return [][]any{}
	}
	tuples, err := parseReadResponseTuples([]byte(raw))
	if err != nil {
		return nil
	}
	if len(tuples) == 0 {
		return [][]any{}
	}
	return tuples
}
