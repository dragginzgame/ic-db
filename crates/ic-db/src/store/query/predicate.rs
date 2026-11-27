//! Residual predicates on a validated row, after physical owner checks.
use super::{
    EntitySchema, Error, IndexValue, MemberNeedle, Predicate, QueryOperand, Term, TermOperand,
    contains_member, scalar_bytes,
};

pub(super) struct PredicateRow<'a> {
    pub entity: &'a EntitySchema,
    pub bytes: &'a [u8],
    pub values: &'a [IndexValue<'a>],
    pub spans: &'a [(u16, u16)],
    pub span_fields: u32,
}
impl<'a> PredicateRow<'a> {
    fn read(
        &self,
        position: usize,
        member: Option<crate::member::RecordMember>,
    ) -> Result<IndexValue<'a>, Error> {
        if let Some(member) = member {
            let (start, end) = self.spans[position];
            member.read(
                &self.bytes[usize::from(start)..usize::from(end)],
                self.entity.fields[position].optional,
            )
        } else {
            Ok(self.values[position])
        }
    }

    fn accepts(
        &self,
        term: &Term,
        supplied: &QueryOperand<'_>,
        component: &mut Vec<u8>,
        right_component: &mut Vec<u8>,
    ) -> Result<bool, Error> {
        let mut value = self.values[term.position];
        component.clear();
        let (operand, optional) = match &term.operand {
            TermOperand::Nested {
                member,
                literal,
                absent,
            } => {
                value = self.read(term.position, Some(*member))?;
                if *absent {
                    return Ok(term.comparison.accepts_absent(value == IndexValue::Absent));
                }
                (
                    Some(literal.as_slice()),
                    self.entity.fields[term.position].optional || member.field().optional,
                )
            }
            TermOperand::Literal { absent: true, .. } => {
                let absent = if self.span_fields & (1 << term.position) != 0 {
                    self.bytes[usize::from(self.spans[term.position].0)] == 0
                } else {
                    value == IndexValue::Absent
                };

                return Ok(term.comparison.accepts_absent(absent));
            }
            TermOperand::Member { item, needle } => {
                let needle = match (needle, supplied) {
                    (MemberNeedle::Literal(_), QueryOperand::Literal(value)) => *value,
                    (MemberNeedle::Field(right, member), QueryOperand::Field(_)) => {
                        self.read(*right, *member)?
                    }
                    _ => return Err(Error::Corrupt),
                };
                if needle == IndexValue::Absent {
                    return Ok(false);
                }
                let (start, end) = self.spans[term.position];

                return contains_member(
                    &self.bytes[usize::from(start)..usize::from(end)],
                    self.entity.fields[term.position].optional,
                    *item,
                    needle,
                );
            }
            TermOperand::Length { member, threshold } => {
                let (start, end) = self.spans[term.position];
                let span = &self.bytes[usize::from(start)..usize::from(end)];
                let optional = self.entity.fields[term.position].optional;
                let actual = if let Some(member) = member {
                    match member.read(span, optional)? {
                        IndexValue::Absent => None,
                        value => Some(scalar_bytes(value).ok_or(Error::Corrupt)?.len() as u32),
                    }
                } else {
                    crate::codec::validated_length(span, optional)
                };

                return Ok(term.comparison.accepts_length(actual, *threshold));
            }
            TermOperand::Literal {
                bytes,
                absent: false,
            } => (
                Some(bytes.as_slice()),
                self.entity.fields[term.position].optional,
            ),
            TermOperand::Field {
                right,
                left_member,
                right_member,
            } => {
                value = self.read(term.position, *left_member)?;
                let rhs = self.read(*right, *right_member)?;
                if value == IndexValue::Absent || rhs == IndexValue::Absent {
                    return Ok(term.comparison.accepts_absent(value == rhs));
                }
                if let Some(matches) = term.comparison.accepts_values(value, rhs) {
                    return Ok(matches);
                }
                right_component.clear();
                rhs.append_component(false, right_component);

                (Some(right_component.as_slice()), false)
            }
        };
        if let QueryOperand::Literal(right) = supplied
            && let Some(matches) = term.comparison.accepts_values(value, *right)
        {
            return Ok(matches);
        }
        value.append_component(optional, component);

        Ok(term.comparison.accepts(
            (value != IndexValue::Absent).then_some(component.as_slice()),
            operand,
            optional,
        ))
    }

    pub(super) fn matches(
        &self,
        terms: &[Term],
        predicates: &[Predicate<'_>],
        component: &mut Vec<u8>,
        right_component: &mut Vec<u8>,
    ) -> Result<bool, Error> {
        let mut clause_matches = false;
        for (term, predicate) in terms.iter().zip(predicates) {
            clause_matches = clause_matches
                || self.accepts(term, &predicate.operand, component, right_component)?;
            if !term.or_next {
                if !clause_matches {
                    return Ok(false);
                }
                clause_matches = false;
            }
        }

        Ok(true)
    }
}
