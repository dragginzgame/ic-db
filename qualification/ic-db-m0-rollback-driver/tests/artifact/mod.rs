//! Local benchmark evidence only; Artifacts owns bounded reads and byte identity.
use ic_host_artifacts::{Identity, Length};
use std::{collections::BTreeSet, error::Error, io::Read};
use wasmparser::{Encoding, Parser, Payload, Validator};

pub struct Facts {
    pub identity: Identity,
    pub code_section_bytes: u64,
    pub exports: BTreeSet<String>,
}

// One policy for the three benchmark families. No partial bytes/facts escape a
// refusal. Full validation follows bounded framing before PocketIC installation.
pub fn read(reader: impl Read) -> Result<(Vec<u8>, Facts), Box<dyn Error>> {
    let (bytes, identity) = ic_host_artifacts::read(reader, Length::AtMost(32 * 1024 * 1024))?;
    let mut code_section_bytes = 0;
    let mut exports = BTreeSet::new();
    let mut sections = 0;
    let mut custom = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        let payload = payload?;
        if payload.as_section().is_some() {
            sections += 1;
            if sections > 128 {
                return Err("benchmark Wasm exceeds 128 sections".into());
            }
        }
        match payload {
            Payload::Version { encoding, .. } if encoding != Encoding::Module => {
                return Err("benchmark requires a core Wasm module".into());
            }
            Payload::CustomSection(_) => {
                custom += 1;
                if custom > 32 {
                    return Err("benchmark Wasm exceeds 32 custom sections".into());
                }
            }
            Payload::ExportSection(reader) => {
                if reader.count() > 128 {
                    return Err("benchmark Wasm exceeds 128 exports".into());
                }
                for export in reader {
                    if !exports.insert(export?.name.to_owned()) {
                        return Err("duplicate benchmark Wasm export".into());
                    }
                }
            }
            // Preserve the former metric: payload including function count,
            // excluding the section identifier and its encoded length.
            Payload::CodeSectionStart {
                unchecked_range: range,
                ..
            } => {
                code_section_bytes = range.end - range.start;
            }
            _ => {}
        }
    }
    Validator::new().validate_all(&bytes)?;
    Ok((
        bytes,
        Facts {
            identity,
            code_section_bytes,
            exports,
        },
    ))
}
