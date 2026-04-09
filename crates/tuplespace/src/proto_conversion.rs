// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Protobuf conversion helpers for tuplespace domain types.
//!
//! These helpers keep protobuf wire decoding/encoding in one place so service
//! controllers and WASM host adapters can delegate to the same mapping logic.

use crate::{Lease, OrderedFloat, Pattern, PatternField, Tuple, TupleField};
use chrono::Utc;
use plexspaces_proto::{
    prost_types,
    tuplespace::v1::{
        tuple_field::Value as ProtoValue, Tuple as ProtoTuple, TupleField as ProtoTupleField,
    },
};

/// Error returned when a protobuf tuple/template cannot be mapped to domain types.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TupleProtoConversionError {
    /// A tuple/template must contain at least one field.
    #[error("{0} must have at least one field")]
    EmptyFields(&'static str),
    /// A tuple field was missing a `oneof` value.
    #[error("TupleField must have a value or wildcard")]
    MissingValue,
    /// Wildcards are only valid in read/take templates, not in tuples written to storage.
    #[error("Wildcard cannot be used as a tuple value (only in patterns)")]
    WildcardInTupleValue,
}

/// Convert a protobuf tuple field into a domain tuple field.
pub fn proto_field_to_tuple_field(
    proto_field: &ProtoTupleField,
) -> Result<TupleField, TupleProtoConversionError> {
    match &proto_field.value {
        Some(ProtoValue::Integer(value)) => Ok(TupleField::Integer(*value)),
        Some(ProtoValue::Float(value)) => Ok(TupleField::Float(OrderedFloat::new(*value))),
        Some(ProtoValue::String(value)) => Ok(TupleField::String(value.clone())),
        Some(ProtoValue::Boolean(value)) => Ok(TupleField::Boolean(*value)),
        Some(ProtoValue::Binary(value)) => Ok(TupleField::Binary(value.clone())),
        Some(ProtoValue::Null(_)) => Ok(TupleField::Null),
        Some(ProtoValue::Wildcard(_)) => Err(TupleProtoConversionError::WildcardInTupleValue),
        None => Err(TupleProtoConversionError::MissingValue),
    }
}

/// Convert a protobuf tuple into a domain tuple.
pub fn proto_tuple_to_tuple(proto_tuple: &ProtoTuple) -> Result<Tuple, TupleProtoConversionError> {
    if proto_tuple.fields.is_empty() {
        return Err(TupleProtoConversionError::EmptyFields("Tuple"));
    }

    let mut tuple = Tuple::new(
        proto_tuple
            .fields
            .iter()
            .map(proto_field_to_tuple_field)
            .collect::<Result<Vec<_>, _>>()?,
    );

    for (key, value) in &proto_tuple.metadata {
        let value_str = match value.kind.as_ref() {
            Some(prost_types::value::Kind::StringValue(inner)) => inner.clone(),
            Some(prost_types::value::Kind::NumberValue(inner)) => inner.to_string(),
            Some(prost_types::value::Kind::BoolValue(inner)) => inner.to_string(),
            Some(prost_types::value::Kind::NullValue(_)) | None => String::new(),
            Some(prost_types::value::Kind::ListValue(_))
            | Some(prost_types::value::Kind::StructValue(_)) => format!("{:?}", value),
        };
        tuple = tuple.with_metadata(key.clone(), value_str);
    }

    if let Some(proto_lease) = &proto_tuple.lease {
        if let Some(ttl) = &proto_lease.ttl {
            let ttl_duration = chrono::Duration::seconds(ttl.seconds)
                + chrono::Duration::nanoseconds(ttl.nanos as i64);
            let mut lease = Lease::new(ttl_duration);
            if !proto_lease.owner.is_empty() {
                lease = lease.with_owner(proto_lease.owner.clone());
            }
            if proto_lease.renewable {
                lease = lease.renewable();
            }
            tuple = tuple.with_lease(lease);
        }
    }

    Ok(tuple)
}

/// Convert a protobuf tuple template into a domain pattern.
pub fn proto_template_to_pattern(
    proto_tuple: &ProtoTuple,
) -> Result<Pattern, TupleProtoConversionError> {
    if proto_tuple.fields.is_empty() {
        return Err(TupleProtoConversionError::EmptyFields("Template"));
    }

    let mut fields = Vec::with_capacity(proto_tuple.fields.len());
    for proto_field in &proto_tuple.fields {
        let field = match &proto_field.value {
            Some(ProtoValue::Wildcard(_)) => PatternField::Wildcard,
            Some(ProtoValue::Integer(value)) => {
                PatternField::Exact(TupleField::Integer(*value))
            }
            Some(ProtoValue::Float(value)) => {
                PatternField::Exact(TupleField::Float(OrderedFloat::new(*value)))
            }
            Some(ProtoValue::String(value)) => {
                PatternField::Exact(TupleField::String(value.clone()))
            }
            Some(ProtoValue::Boolean(value)) => {
                PatternField::Exact(TupleField::Boolean(*value))
            }
            Some(ProtoValue::Binary(value)) => {
                PatternField::Exact(TupleField::Binary(value.clone()))
            }
            Some(ProtoValue::Null(_)) => PatternField::Exact(TupleField::Null),
            None => return Err(TupleProtoConversionError::MissingValue),
        };
        fields.push(field);
    }

    Ok(Pattern::new(fields))
}

/// Convert a domain tuple field into a protobuf tuple field.
pub fn tuple_field_to_proto_field(field: &TupleField) -> ProtoTupleField {
    let value = match field {
        TupleField::Integer(value) => Some(ProtoValue::Integer(*value)),
        TupleField::Float(value) => Some(ProtoValue::Float(value.get())),
        TupleField::String(value) => Some(ProtoValue::String(value.clone())),
        TupleField::Boolean(value) => Some(ProtoValue::Boolean(*value)),
        TupleField::Binary(value) => Some(ProtoValue::Binary(value.clone())),
        TupleField::Null => Some(ProtoValue::Null(true)),
    };
    ProtoTupleField { value }
}

/// Convert a domain tuple into a protobuf tuple.
pub fn tuple_to_proto_tuple(tuple: &Tuple) -> ProtoTuple {
    let lease = tuple.lease().map(|lease| {
        let expires_at = lease.expires_at();
        let now = Utc::now();
        let ttl_duration = if expires_at > now {
            expires_at - now
        } else {
            chrono::Duration::zero()
        };

        plexspaces_proto::tuplespace::v1::Lease {
            ttl: Some(prost_types::Duration {
                seconds: ttl_duration.num_seconds(),
                nanos: (ttl_duration.num_nanoseconds().unwrap_or(0) % 1_000_000_000) as i32,
            }),
            owner: lease.owner().cloned().unwrap_or_default(),
            renewable: lease.is_renewable(),
            expires_at: Some(prost_types::Timestamp {
                seconds: expires_at.timestamp(),
                nanos: expires_at.timestamp_subsec_nanos() as i32,
            }),
        }
    });

    let now = chrono::Utc::now();
    let timestamp = Some(prost_types::Timestamp {
        seconds: now.timestamp(),
        nanos: now.timestamp_subsec_nanos() as i32,
    });

    ProtoTuple {
        id: ulid::Ulid::new().to_string(),
        fields: tuple.fields().iter().map(tuple_field_to_proto_field).collect(),
        timestamp,
        lease,
        metadata: Default::default(),
        location: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::tuplespace::v1::tuple_field::Value;

    #[test]
    fn converts_tuple_and_template_round_trip() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![
                ProtoTupleField {
                    value: Some(Value::String("topic".to_string())),
                },
                ProtoTupleField {
                    value: Some(Value::Integer(42)),
                },
                ProtoTupleField {
                    value: Some(Value::Wildcard(true)),
                },
            ],
            timestamp: None,
            lease: None,
            metadata: Default::default(),
            location: None,
        };

        let pattern = proto_template_to_pattern(&proto_tuple).expect("template should convert");
        assert_eq!(pattern.fields().len(), 3);

        let tuple = Tuple::new(vec![
            TupleField::String("topic".to_string()),
            TupleField::Integer(42),
            TupleField::Null,
        ]);
        let encoded = tuple_to_proto_tuple(&tuple);
        let decoded = proto_tuple_to_tuple(&encoded).expect("tuple should convert");
        assert_eq!(decoded.fields(), tuple.fields());
    }
}
