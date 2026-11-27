//! Query continuation frames, binding to an admitted traversal, and position validation.

use super::{
    ENTITY_PREFIX_BYTES, INDEX_PREFIX_BYTES, MemberNeedle, Plan, Projection, Request, TermOperand,
    consume_index_component, distinct,
};
use crate::{EntitySchema, Error, key, primary::RowKey};
use sha2::{Digest, Sha256};

/// A bounded logical position, bound to database, schema, query and direction.
/// It is not an authorization token or a snapshot. Callers may choose positions;
/// the database revalidates all claims and retains no trusted progress counters.
/// A new deployment may invalidate a previously issued cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryCursor {
    binding: [u8; 32],
    pub(super) position: Vec<u8>,
    pub(super) resume: Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Resume {
    After = 0,
    Retry = 1,
    AfterUnemittedClass = 2,
}
impl QueryCursor {
    /// Maximum length of a serialized query cursor, in bytes.
    pub const MAX_ENCODED_BYTES: usize = 34 + crate::MAX_INDEX_KEY_BYTES;

    /// Encode the bounded continuation for an application's transport.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(34 + self.position.len());
        bytes.extend([1, self.resume as u8]);
        bytes.extend(self.binding);
        bytes.extend(&self.position);

        bytes
    }

    /// Decode framing; execution additionally checks query identity and key shape.
    /// # Errors
    /// Refuses unsupported versions, flags and oversized or incomplete frames.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if !(35..=Self::MAX_ENCODED_BYTES).contains(&bytes.len()) || bytes[0] != 1 {
            return Err(Error::InvalidCursor);
        }

        Ok(Self {
            binding: bytes[2..34].try_into().map_err(|_| Error::InvalidCursor)?,
            position: bytes[34..].to_vec(),
            resume: match bytes[1] {
                0 => Resume::After,
                1 => Resume::Retry,
                2 => Resume::AfterUnemittedClass,
                _ => return Err(Error::InvalidCursor),
            },
        })
    }
}

impl serde::Serialize for QueryCursor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.to_bytes())
    }
}

impl<'de> serde::Deserialize<'de> for QueryCursor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Frame;

        impl<'de> serde::de::Visitor<'de> for Frame {
            type Value = QueryCursor;

            fn expecting(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                fmt.write_str("a bounded IC DB query cursor frame")
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<QueryCursor, E> {
                QueryCursor::from_bytes(bytes).map_err(E::custom)
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<QueryCursor, A::Error> {
                let size = seq.size_hint().unwrap_or(0);
                if size > QueryCursor::MAX_ENCODED_BYTES {
                    return Err(serde::de::Error::custom(Error::InvalidCursor));
                }
                let mut frame = Vec::with_capacity(size);
                while let Some(byte) = seq.next_element()? {
                    if frame.len() == QueryCursor::MAX_ENCODED_BYTES {
                        return Err(serde::de::Error::custom(Error::InvalidCursor));
                    }
                    frame.push(byte);
                }
                QueryCursor::from_bytes(&frame).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_bytes(Frame)
    }
}

impl Plan<'_> {
    pub(super) fn bind_cursor(
        &mut self,
        entity: &EntitySchema,
        request: &Request<'_>,
        schema: [u8; 32],
        incarnation: [u8; 16],
        projection: Option<Projection<'_>>,
    ) -> Result<(), Error> {
        let mut hash = Sha256::new();
        hash.update(b"ic-db:query:v1\0");
        hash.update(schema);
        hash.update(incarnation);
        hash.update([u8::from(request.descending)]);
        hash.update((self.prefix.len() as u32).to_le_bytes());
        hash.update(&self.prefix);
        for term in &self.terms {
            hash.update([term.position as u8, term.comparison as u8]);

            match &term.operand {
                TermOperand::Nested {
                    member,
                    literal,
                    absent,
                } => {
                    hash.update((u32::MAX - 2).to_le_bytes());
                    hash.update([member.position() as u8, u8::from(*absent)]);
                    // The binding already records absence separately from the scalar payload.
                    let literal = &literal[usize::from(
                        entity.fields[term.position].optional || member.field().optional,
                    )..];
                    hash.update((literal.len() as u32).to_le_bytes());
                    hash.update(literal);
                }
                TermOperand::Literal { bytes, .. }
                | TermOperand::Member {
                    needle: MemberNeedle::Literal(bytes),
                    ..
                } => {
                    hash.update((bytes.len() as u32).to_le_bytes());
                    hash.update(bytes);
                }
                TermOperand::Length { member, threshold } => {
                    if let Some(member) = member {
                        hash.update((u32::MAX - 3).to_le_bytes());
                        hash.update([member.position() as u8]);
                    } else {
                        hash.update((u32::MAX - 1).to_le_bytes());
                    }
                    hash.update(threshold.to_le_bytes());
                }
                TermOperand::Field {
                    right,
                    left_member: None,
                    right_member: None,
                }
                | TermOperand::Member {
                    needle: MemberNeedle::Field(right, None),
                    ..
                } => {
                    // Outside the bounded literal length domain.
                    hash.update(u32::MAX.to_le_bytes());
                    hash.update([*right as u8]);
                }
                TermOperand::Member {
                    needle: MemberNeedle::Field(right, Some(member)),
                    ..
                } => {
                    hash.update((u32::MAX - 4).to_le_bytes());
                    hash.update([u8::MAX, *right as u8, member.position() as u8]);
                }
                TermOperand::Field {
                    right,
                    left_member,
                    right_member,
                } => {
                    hash.update((u32::MAX - 4).to_le_bytes());
                    let slot = |member: &Option<crate::member::RecordMember>| {
                        member.map_or(u8::MAX, |m| m.position() as u8)
                    };
                    hash.update([slot(left_member), *right as u8, slot(right_member)]);
                }
            }
        }
        // The clause mask records which adjacent terms are joined by OR.
        let clauses = self
            .terms
            .iter()
            .enumerate()
            .fold(0_u8, |mask, (i, term)| mask | (u8::from(term.or_next) << i));
        if clauses != 0 {
            hash.update(b"or-clauses\0");
            hash.update([clauses]);
        }
        if let Some(order) = &request.order {
            hash.update(b"order\0");
            hash.update([order.len() as u8]);
            let mut directions = 0u8;
            for (i, &super::OrderTerm(name, direction)) in order.iter().enumerate() {
                if direction == super::SortDirection::Descending {
                    directions |= 1 << i;
                }
                // Field names were admitted by order selection above.
                if let Some(position) = entity.field_position(name) {
                    hash.update([position as u8]);
                } else {
                    // A top-level position is at most 31. Frame each admitted nested path.
                    hash.update([u8::MAX, name.len() as u8]);
                    hash.update(name.as_bytes());
                }
            }
            if directions != 0 {
                hash.update(b"order-directions\0");
                hash.update([directions]);
            }
        }
        // Projected shapes bind their field selection and column order.
        if let Some(mask) = self.projection {
            hash.update(b"projection\0");
            hash.update(mask.to_le_bytes());
            if let Some(Projection::Columns(columns)) = projection {
                hash.update(b"columns\0");
                hash.update(columns);
            }
        }
        if request.distinct {
            hash.update(b"distinct\0");
        }
        self.binding = hash.finalize().into();
        if let Some(cursor) = request.cursor {
            self.validate_cursor(entity, cursor)
                .map_err(|_| Error::InvalidCursor)?;
        }

        Ok(())
    }

    pub(super) fn validate_cursor(
        &self,
        entity: &EntitySchema,
        cursor: &QueryCursor,
    ) -> Result<(), Error> {
        // A mismatched persisted job cursor is corrupt. Caller-supplied cursors
        // map every validation failure to InvalidCursor at admission.
        if cursor.binding != self.binding {
            return Err(Error::Corrupt);
        }
        if cursor.resume == Resume::AfterUnemittedClass
            && !matches!(self.distinct, distinct::Strategy::Adjacent(_))
        {
            return Err(Error::InvalidCursor);
        }
        let bytes = cursor.position.as_slice();
        if bytes.len() > crate::MAX_INDEX_KEY_BYTES || !bytes.starts_with(&self.prefix) {
            return Err(Error::InvalidCursor);
        }
        if let Some(index) = self.index {
            let mut remaining = bytes
                .get(INDEX_PREFIX_BYTES..)
                .ok_or(Error::InvalidCursor)?;
            for name in index.fields {
                consume_index_component(entity, index, name, &mut remaining)?;
            }
            if !index.unique {
                key::consume(&mut remaining, entity.key_type()?)?;
            }
            if !remaining.is_empty() {
                return Err(Error::InvalidCursor);
            }
        } else {
            RowKey::decode(
                bytes
                    .get(ENTITY_PREFIX_BYTES..)
                    .ok_or(Error::InvalidCursor)?,
                entity.key_type()?,
            )?;
        }

        Ok(())
    }

    pub(super) fn cursor(&self, position: &[u8], resume: Resume) -> QueryCursor {
        QueryCursor {
            binding: self.binding,
            position: position.to_vec(),
            resume,
        }
    }
}
