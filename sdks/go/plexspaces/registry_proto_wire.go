// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Protobuf wire encoding/decoding for plexspaces.object_registry.v1 request/response
// messages used by WASM host imports. Hand-coded to avoid importing generated proto
// packages in TinyGo WASM builds. Matches prost output in crates/wasm-runtime.
//
// Field numbers are taken directly from proto/plexspaces/v1/registry/object_registry.proto.

package plexspaces

// ============================================================================
// ObjectRegistration encoder
// Field numbers: object_id=1, object_name=2, object_type=3, tenant_id=5,
//                namespace=6, grpc_address=8, object_category=9,
//                capabilities=10 (repeated string), labels=13 (repeated string),
//                alias=18
// ============================================================================

func encodeObjectRegistration(reg ObjectRegistration) []byte {
	var b []byte
	b = appendStringField(b, 1, reg.ObjectID)
	if reg.ObjectType != "" {
		ot := objectTypeFromString(reg.ObjectType)
		if ot != 0 {
			b = appendVarintField(b, 3, uint64(ot))
		}
	}
	if reg.GRPCAddress != "" {
		b = appendStringField(b, 8, reg.GRPCAddress)
	}
	if reg.ObjectCategory != "" {
		b = appendStringField(b, 9, reg.ObjectCategory)
	}
	if reg.TenantID != "" {
		b = appendStringField(b, 5, reg.TenantID)
	}
	if reg.Namespace != "" {
		b = appendStringField(b, 6, reg.Namespace)
	}
	for _, cap := range reg.Capabilities {
		b = appendStringField(b, 10, cap)
	}
	for _, lbl := range reg.Labels {
		b = appendStringField(b, 13, lbl)
	}
	if reg.Alias != nil && *reg.Alias != "" {
		b = appendStringField(b, 18, *reg.Alias)
	}
	return b
}

// objectTypeFromString maps ObjectRegistration.ObjectType string to proto enum value.
func objectTypeFromString(s string) int32 {
	switch s {
	case "actor", "OBJECT_TYPE_ACTOR":
		return 1
	case "tuplespace", "OBJECT_TYPE_TUPLESPACE":
		return 2
	case "service", "OBJECT_TYPE_SERVICE":
		return 3
	case "vm", "OBJECT_TYPE_VM":
		return 4
	case "application", "OBJECT_TYPE_APPLICATION":
		return 5
	case "workflow", "OBJECT_TYPE_WORKFLOW":
		return 6
	case "node", "OBJECT_TYPE_NODE":
		return 7
	case "process_group", "OBJECT_TYPE_PROCESS_GROUP":
		return 8
	}
	return 0
}

func objectTypeToString(n int32) string {
	switch n {
	case 1:
		return "actor"
	case 2:
		return "tuplespace"
	case 3:
		return "service"
	case 4:
		return "vm"
	case 5:
		return "application"
	case 6:
		return "workflow"
	case 7:
		return "node"
	case 8:
		return "process_group"
	}
	return ""
}

// ============================================================================
// RegisterRequest: registration=1 (message)
// ============================================================================

func encodeRegisterRequest(reg ObjectRegistration) []byte {
	inner := encodeObjectRegistration(reg)
	return appendLengthDelimited(nil, 1, inner)
}

// ============================================================================
// UnregisterRequest: object_id=1, object_type=2, tenant_id=3, namespace=4
// ============================================================================

func encodeUnregisterRequest(objectID string, objectType int32, tenantID, namespace string) []byte {
	var b []byte
	b = appendStringField(b, 1, objectID)
	if objectType != 0 {
		b = appendVarintField(b, 2, uint64(objectType))
	}
	if tenantID != "" {
		b = appendStringField(b, 3, tenantID)
	}
	if namespace != "" {
		b = appendStringField(b, 4, namespace)
	}
	return b
}

// ============================================================================
// LookupRequest: object_id=1, object_type=2, tenant_id=3, namespace=4, alias=5
// ============================================================================

func encodeLookupRequest(objectID string, objectType int32, tenantID, namespace, alias string) []byte {
	var b []byte
	if objectID != "" {
		b = appendStringField(b, 1, objectID)
	}
	if objectType != 0 {
		b = appendVarintField(b, 2, uint64(objectType))
	}
	if tenantID != "" {
		b = appendStringField(b, 3, tenantID)
	}
	if namespace != "" {
		b = appendStringField(b, 4, namespace)
	}
	if alias != "" {
		b = appendStringField(b, 5, alias)
	}
	return b
}

// ============================================================================
// DiscoverRequest: object_type=1, object_category=2, tenant_id=4, namespace=5,
//                  capabilities=6, labels=7, page_size=10
// ============================================================================

func encodeDiscoverRequest(objectType int32, objectCategory, tenantID, namespace string,
	capabilities, labels []string, pageSize int32) []byte {
	var b []byte
	if objectType != 0 {
		b = appendVarintField(b, 1, uint64(objectType))
	}
	if objectCategory != "" {
		b = appendStringField(b, 2, objectCategory)
	}
	if tenantID != "" {
		b = appendStringField(b, 4, tenantID)
	}
	if namespace != "" {
		b = appendStringField(b, 5, namespace)
	}
	for _, cap := range capabilities {
		b = appendStringField(b, 6, cap)
	}
	for _, lbl := range labels {
		b = appendStringField(b, 7, lbl)
	}
	if pageSize > 0 {
		b = appendVarintField(b, 10, uint64(pageSize))
	}
	return b
}

// ============================================================================
// HeartbeatRequest: object_id=1, object_type=2, tenant_id=3, namespace=4
// ============================================================================

func encodeHeartbeatRequest(objectID string, objectType int32, tenantID, namespace string) []byte {
	var b []byte
	b = appendStringField(b, 1, objectID)
	if objectType != 0 {
		b = appendVarintField(b, 2, uint64(objectType))
	}
	if tenantID != "" {
		b = appendStringField(b, 3, tenantID)
	}
	if namespace != "" {
		b = appendStringField(b, 4, namespace)
	}
	return b
}

// ============================================================================
// LookupResponse decoder: registration=1 (message), found=2 (bool)
// ============================================================================

func decodeLookupResponse(data []byte) (*ObjectRegistration, bool) {
	pos := 0
	var regBytes []byte
	found := false
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2: // registration message
			ln, m, err := readVarint(data, pos)
			if err != nil {
				break
			}
			pos += m
			end := pos + int(ln)
			if end > len(data) {
				break
			}
			regBytes = data[pos:end]
			pos = end
		case fn == 2 && wt == 0: // found bool
			v, m, err := readVarint(data, pos)
			if err != nil {
				break
			}
			pos += m
			found = v != 0
		default:
			var err error
			pos, err = skipField(data, pos, wt)
			if err != nil {
				break
			}
		}
	}
	if !found || len(regBytes) == 0 {
		return nil, false
	}
	reg := decodeObjectRegistration(regBytes)
	return &reg, true
}

// ============================================================================
// DiscoverResponse decoder: registrations=1 (repeated message)
// ============================================================================

func decodeDiscoverResponse(data []byte) []ObjectRegistration {
	var results []ObjectRegistration
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 1 && wt == 2 {
			ln, m, err := readVarint(data, pos)
			if err != nil {
				break
			}
			pos += m
			end := pos + int(ln)
			if end > len(data) {
				break
			}
			reg := decodeObjectRegistration(data[pos:end])
			results = append(results, reg)
			pos = end
		} else {
			var err error
			pos, err = skipField(data, pos, wt)
			if err != nil {
				break
			}
		}
	}
	return results
}

// ============================================================================
// ObjectRegistration decoder
// ============================================================================

func decodeObjectRegistration(data []byte) ObjectRegistration {
	var reg ObjectRegistration
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		switch {
		case wt == 2: // length-delimited (string, bytes, embedded message)
			ln, m, err := readVarint(data, pos)
			if err != nil {
				break
			}
			pos += m
			end := pos + int(ln)
			if end > len(data) {
				break
			}
			chunk := string(data[pos:end])
			pos = end
			switch fn {
			case 1:
				reg.ObjectID = chunk
			case 2:
				// object_name - ignore
			case 5:
				reg.TenantID = chunk
			case 6:
				reg.Namespace = chunk
			case 8:
				reg.GRPCAddress = chunk
			case 9:
				reg.ObjectCategory = chunk
			case 10:
				reg.Capabilities = append(reg.Capabilities, chunk)
			case 13:
				reg.Labels = append(reg.Labels, chunk)
			case 18:
				alias := chunk
				reg.Alias = &alias
			}
		case wt == 0: // varint
			v, m, err := readVarint(data, pos)
			if err != nil {
				break
			}
			pos += m
			switch fn {
			case 3: // object_type enum
				reg.ObjectType = objectTypeToString(int32(v))
			case 12: // health_status enum
				// ignore for now
			}
		default:
			var err error
			pos, err = skipField(data, pos, wt)
			if err != nil {
				break
			}
		}
	}
	return reg
}

// ============================================================================
// Low-level proto encoding helpers (wire format)
// appendVarint/appendLengthDelimited defined in tuplespace_proto_wire.go
// ============================================================================

func appendStringField(buf []byte, fieldNum int, s string) []byte {
	tag := uint64(fieldNum<<3 | 2) // wire type 2 = length-delimited
	buf = appendVarint(buf, tag)
	buf = appendVarint(buf, uint64(len(s)))
	return append(buf, s...)
}

func appendVarintField(buf []byte, fieldNum int, v uint64) []byte {
	tag := uint64(fieldNum << 3) // wire type 0 = varint
	buf = appendVarint(buf, tag)
	return appendVarint(buf, v)
}
