//! Adjacent projected equality, proven from the existing physical index order.
use super::{Request, consume_index_component};
use crate::{EntitySchema, Error, IndexSchema};

// None means identity: a complete entity or a projection containing its unique PK.
// Some(n) selects the first n physical index components as the equivalence key.
pub(super) fn admit(
    entity: &EntitySchema,
    request: &Request<'_>,
    projection: Option<u16>,
    index: Option<&IndexSchema>,
    fixed: usize,
) -> Result<Option<usize>, Error> {
    let Some(mut remaining) = projection else {
        return Ok(None);
    };
    let position = |name: &str| {
        entity
            .fields
            .iter()
            .position(|f| f.name == name)
            .ok_or(Error::InvalidQuery)
    };
    if remaining & (1 << position(entity.primary_key)?) != 0 {
        return Ok(None);
    }
    let index = index.ok_or(Error::InvalidQuery)?;
    // Every predicate must have the same answer throughout a projected class.
    // A class is then either wholly rejected or emitted before an exclusive
    // continuation can advance into it. Other filters need additional state.
    for predicate in &request.predicates {
        if remaining & (1 << position(predicate.field)?) == 0
            && !index.fields[..fixed].contains(&predicate.field)
        {
            return Err(Error::InvalidQuery);
        }
    }
    for name in &index.fields[..fixed] {
        remaining &= !(1 << position(name)?);
    }
    if remaining == 0 {
        return Ok(Some(fixed));
    }
    for (i, name) in index.fields.iter().enumerate().skip(fixed) {
        let bit = 1 << position(name)?;
        if remaining & bit == 0 {
            return Err(Error::InvalidQuery);
        }
        remaining &= !bit;
        if remaining == 0 {
            return Ok(Some(i + 1));
        }
    }
    Err(Error::InvalidQuery)
}

pub(super) fn boundary<'a>(
    entity: &EntitySchema,
    index: Option<&IndexSchema>,
    components: Option<usize>,
    position: &'a [u8],
) -> Result<Option<&'a [u8]>, Error> {
    let Some(components) = components else {
        return Ok(None);
    };
    let index = index.ok_or(Error::InvalidQuery)?;
    let mut remaining = position.get(9..).ok_or(Error::InvalidCursor)?;
    for name in index.fields.iter().take(components) {
        consume_index_component(entity, index, name, &mut remaining)?;
    }
    Ok(Some(&position[..position.len() - remaining.len()]))
}
