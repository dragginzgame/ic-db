//! Resolve a scalar member once and read it from a validated record span.

use crate::{Error, FieldSchema, FieldType, IndexValue, TypeBody, codec::Decoder};

#[derive(Clone, Copy)]
pub(crate) struct RecordMember {
    fields: &'static [FieldSchema],
    position: usize,
}

// Resolve the author-facing path before either filter or order admission.
pub(crate) fn resolve_field(
    entity: &crate::EntitySchema,
    path: &str,
) -> Result<(usize, Option<RecordMember>), Error> {
    if path.len() > 64 + 1 + 64 {
        return Err(Error::InvalidQuery);
    }
    if path.contains('.') {
        let (position, member) = entity.record_member(path).ok_or(Error::InvalidQuery)?;
        Ok((position, Some(member)))
    } else {
        Ok((
            entity.field_position(path).ok_or(Error::InvalidQuery)?,
            None,
        ))
    }
}

impl RecordMember {
    pub(crate) fn resolve(parent: FieldType, name: &str) -> Result<Self, Error> {
        let FieldType::Named(link) = parent else {
            return Err(Error::InvalidQuery);
        };
        let TypeBody::Record(fields) = (link.definition)().body else {
            return Err(Error::InvalidQuery);
        };
        let position = fields
            .iter()
            .position(|f| f.name == name)
            .ok_or(Error::InvalidQuery)?;
        if !crate::key::supports(fields[position].value) {
            return Err(Error::InvalidQuery);
        }
        Ok(Self { fields, position })
    }

    pub(crate) fn position(&self) -> usize {
        self.position
    }

    pub(crate) fn field(&self) -> &'static FieldSchema {
        &self.fields[self.position]
    }

    pub(crate) fn read<'a>(&self, span: &'a [u8], optional: bool) -> Result<IndexValue<'a>, Error> {
        let mut reader = Decoder::new(span)?;
        if optional && !reader.boolean()? {
            return Ok(IndexValue::Absent);
        }
        for sibling in &self.fields[..self.position] {
            if !sibling.optional || reader.boolean()? {
                reader.skip(sibling.value)?;
            }
        }
        let leaf = self.field();
        if leaf.optional && !reader.boolean()? {
            return Ok(IndexValue::Absent);
        }
        IndexValue::read(&mut reader, leaf.value)
    }
}
