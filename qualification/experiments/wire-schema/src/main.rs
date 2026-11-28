//! Downstream application payload and host inspection probe.
#![forbid(unsafe_code)]

#[cfg(test)]
use canic::wire::{self, Encoding, Limits};
use ic_db::design::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct UserAdminRequest {
    user_id: Ulid,
    action: Action,
}

#[enum_(variant(name = "Suspend"), variant(name = "Allow"))]
struct Action {}

#[newtype(item(prim = "Nat16"))]
struct Code {}

#[list(item(is = "Code"), max_items = 4)]
struct Codes {}

#[set(item(prim = "Nat8"), max_items = 4)]
struct Roles {}

#[map(key(prim = "Nat8"), value(item(prim = "Nat16")), max_items = 4)]
struct Scores {}

#[record(fields(
    field(name = "scores", value(item(is = "Scores"))),
    field(name = "roles", value(item(is = "Roles"))),
    field(name = "codes", value(item(is = "Codes"))),
    field(name = "wide", value(item(prim = "U256")))
))]
struct Profile {}

#[canister]
struct Directory {}

#[store(canister = Directory, key = "probe.profile.v1")]
struct Profiles {}

#[entity(store = Profiles, pk(field = "id"), fields(
    field(name = "profile", value(item(is = "Profile"))),
    field(name = "id", value(item(prim = "Ulid")))
))]
struct User {}

#[cfg(test)]
const LIMITS: Limits = Limits {
    bytes: 2048,
    allocation: 65536,
    nodes: 256,
    depth: 16,
};

fn payload<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).unwrap()
}

fn main() {
    checked_text::check_payloads();
    let bytes = core::array::from_fn(|i| i as u8);
    for (tag, action) in [(0, Action::Allow), (1, Action::Suspend)] {
        let request = UserAdminRequest {
            user_id: Ulid::from_bytes(bytes),
            action,
        };
        let expected = [bytes.as_slice(), &[tag]].concat();
        assert_eq!(payload(&request), expected);
        assert_eq!(
            postcard::from_bytes::<UserAdminRequest>(&expected).unwrap(),
            request
        );
    }
    let user = User {
        id: Ulid::from_bytes(bytes),
        profile: Profile {
            codes: Codes(vec![Code(130)]),
            roles: Roles([5, 2].into()),
            scores: Scores([(4, 150)].into()),
            wide: U256::from(255_u8),
        },
    };
    // Entity fields follow authored order; record fields and enum tags follow generated order.
    let expected = [
        &[1, 130, 1, 2, 2, 5, 1, 4, 150, 1, 4, b'0', b'x', b'f', b'f'][..],
        &bytes,
    ]
    .concat();
    assert_eq!(payload(&user), expected);
    assert_eq!(postcard::from_bytes::<User>(&expected).unwrap(), user);
}

#[cfg(test)]
mod tests {
    use super::*;
    use canic::wire::typescript::{Method, PayloadLimits, SchemaLimits, SchemaRegistry, Wire};

    type CursorBytes = wire::Bytes<{ ic_db::QueryCursor::MAX_ENCODED_BYTES }>;

    #[derive(Serialize, Deserialize, Wire)]
    #[wire(crate = "canic::wire")]
    struct PageRequest {
        after: Option<CursorBytes>,
    }

    #[derive(Serialize, Deserialize, Wire)]
    #[wire(crate = "canic::wire")]
    struct PageReply {
        id: [u8; 16],
        next: Option<CursorBytes>,
    }

    // Canic supplies the sparse root around a mounted method's Serde body.
    #[derive(Serialize, Deserialize, wire::WireSchema)]
    #[wire(crate = "canic::wire")]
    struct MountedPage {
        id: [u8; 16],
        after: Option<CursorBytes>,
    }

    #[canic::query]
    fn mounted_page(page: MountedPage) -> MountedPage {
        page
    }

    #[test]
    fn mounted_page_inspects_the_fixed_key_and_bounded_cursor() {
        let methods =
            canic::inspection::methods(env!("CARGO_CRATE_NAME"), &Default::default()).unwrap();
        let method = &methods["mounted_page"];
        assert_eq!(method.request.schema, method.reply.schema);
        let graph = &method.request.schema;
        fn resolve<'a>(
            graph: &'a serde_json::Value,
            mut node: &'a serde_json::Value,
        ) -> &'a serde_json::Value {
            for _ in 0..graph.as_object().unwrap().len() {
                if node["k"] != "ref" {
                    return node;
                }
                node = &graph[node["name"].as_str().unwrap()];
            }
            panic!("cyclic schema reference");
        }
        let root = &graph["Root"];
        assert_eq!(root["k"], "extension-tuple");
        let fields = &resolve(graph, &root["items"][0])["fields"];
        assert_eq!(fields[0]["name"], "id");
        let id = resolve(graph, &fields[0]["value"]);
        assert_eq!(id["k"], "array");
        assert_eq!(id["max"], 16);
        assert_eq!(
            resolve(graph, &id["item"]),
            &serde_json::json!({"k":"u","bits":8})
        );
        assert_eq!(fields[1]["name"], "after");
        let cursor = resolve(graph, &fields[1]["value"]);
        assert_eq!(cursor["k"], "option");
        assert_eq!(
            resolve(graph, &cursor["item"]),
            &serde_json::json!({"k":"bytes","max":ic_db::QueryCursor::MAX_ENCODED_BYTES})
        );
    }

    #[test]
    fn checked_principal_uses_the_principal_payload_and_rejects_anonymous() {
        #[newtype(item(prim = "Principal"), validate(non_anonymous))]
        struct ProjectOwner {}

        for principal in [
            Principal::management_canister(),
            Principal::from_slice(&[1, 2, 3]),
        ] {
            let bytes = payload(&principal);
            let owner = ProjectOwner::try_from(principal).unwrap();
            assert_eq!(payload(&owner), bytes);
            assert_eq!(postcard::from_bytes::<ProjectOwner>(&bytes).unwrap(), owner);
        }
        let bytes = payload(&Principal::anonymous());
        assert!(postcard::from_bytes::<ProjectOwner>(&bytes).is_err());
        assert_eq!(
            postcard::from_bytes::<Principal>(&bytes).unwrap(),
            Principal::anonymous()
        );
    }

    #[test]
    fn opaque_cursor_resumes_through_a_generated_sparse_client() {
        use ic_db::{Batch, Entity, QueryCursor, Schema, testing::Fixture};

        let fixture = Fixture::new("test.cursor.v1");
        let mut db = fixture
            .create(Schema::new(&[User::SCHEMA]).unwrap(), [1; 16])
            .unwrap();
        let mut batch = Batch::new();
        for id in 1..=2 {
            batch
                .insert(&User {
                    id: Ulid::from_u128(id),
                    profile: Profile {
                        codes: Codes(vec![]),
                        roles: Roles(Default::default()),
                        scores: Scores(Default::default()),
                        wide: U256::ZERO,
                    },
                })
                .unwrap();
        }
        db.commit(batch).unwrap();
        let query = || db.query::<User>().limit(1);
        let first = query().fetch().unwrap();
        let cursor = first.next.unwrap();
        assert_eq!(first.rows[0].id, Ulid::from_u128(1));
        let id = first.rows[0].id;
        let json = serde_json::to_value(cursor.to_bytes()).unwrap();
        assert_eq!(serde_json::to_value(&cursor).unwrap(), json);
        assert_eq!(serde_json::from_value::<QueryCursor>(json).unwrap(), cursor);
        let mut bad_version = cursor.to_bytes();
        bad_version[0] = 0;
        let mut bad_flag = cursor.to_bytes();
        bad_flag[1] = 3;
        for bad in [bad_version, bad_flag, vec![1; 34], vec![1; 1024]] {
            let limits = Limits {
                nodes: 8192,
                allocation: 1 << 20,
                ..LIMITS
            };
            let oversized = bad.len() > QueryCursor::MAX_ENCODED_BYTES;
            let encoded =
                wire::encode(&Some(wire::ByteBuf::from(bad)), Encoding::Payload, limits).unwrap();
            let decoded = wire::decode::<PageRequest>(&encoded, Encoding::Payload, limits);
            if oversized {
                assert!(decoded.is_err());
            } else {
                let decoded = decoded.unwrap();
                assert!(QueryCursor::from_bytes(decoded.after.unwrap().as_slice()).is_err());
            }
        }
        let limits = PayloadLimits {
            bytes: 2048,
            nodes: 256,
            depth: 16,
        };
        let mut registry = SchemaRegistry::new(SchemaLimits::default()).unwrap();
        registry.register::<PageRequest>("PageRequest").unwrap();
        registry.register::<PageReply>("PageReply").unwrap();
        let registry = registry.finish().unwrap();
        assert!(
            registry
                .declarations()
                .unwrap()
                .contains("globalThis.Uint8Array")
        );
        let method = Method {
            export: "page",
            id: 1,
            request: "PageRequest",
            response: "PageReply",
            request_limits: limits,
            response_limits: limits,
        };
        let module = registry
            .method_module(&[method], "./runtime.ts", &Default::default(), |report| {
                assert!(report.is_clean(), "{report}");
            })
            .unwrap();
        let frame = |direction| Encoding::Frame {
            method: 1,
            direction,
        };
        let response = frame(wire::Direction::Response);
        let request = frame(wire::Direction::Request);
        assert_eq!(
            wire::encode(&PageRequest { after: None }, request, LIMITS).unwrap(),
            wire::encode(&None::<Vec<u8>>, request, LIMITS).unwrap()
        );
        let byte_vector_reply = wire::encode(
            &(id.to_bytes(), Some(wire::ByteBuf::from(cursor.to_bytes()))),
            response,
            LIMITS,
        )
        .unwrap();
        let first_reply = wire::encode(
            &PageReply {
                id: id.to_bytes(),
                next: Some(CursorBytes::new(cursor.to_bytes()).unwrap()),
            },
            response,
            LIMITS,
        )
        .unwrap();
        assert_eq!(first_reply, byte_vector_reply);
        let received: PageReply = wire::decode(&first_reply, response, LIMITS).unwrap();
        let key = ic_db::Id::<User>::from_key(Ulid::from_bytes(received.id));
        assert_eq!(db.get::<User>(key).unwrap(), Some(first.rows[0].clone()));
        assert_eq!(first_reply.last(), Some(&0));
        assert!(
            wire::decode::<PageReply>(&first_reply[..first_reply.len() - 1], response, LIMITS)
                .is_err()
        );
        let next_request = typescript_continue(&module, &first_reply);
        assert_eq!(
            next_request,
            wire::encode(
                &Some(wire::ByteBuf::from(cursor.to_bytes())),
                request,
                LIMITS
            )
            .unwrap()
        );
        let decoded: PageRequest = wire::decode(&next_request, request, LIMITS).unwrap();
        let second = query()
            .after(&QueryCursor::from_bytes(decoded.after.unwrap().as_slice()).unwrap())
            .fetch()
            .unwrap();
        assert_eq!(second.rows[0].id, Ulid::from_u128(2));
        assert_eq!(second.rows.len(), 1);
        assert!(wire::decode::<PageRequest>(&[1, 50, 1], Encoding::Payload, LIMITS).is_err());
    }

    fn typescript_continue(module: &str, reply: &[u8]) -> Vec<u8> {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };

        let mut child = Command::new("node")
            .arg("--experimental-transform-types")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/cursor-client.mjs"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = serde_json::json!({"module":module, "runtime":wire::typescript::RUNTIME, "reply":reply});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    #[test]
    fn downstream_payloads() {
        main();
        for (wide, hex) in [
            (U256::ZERO, "0x0"),
            (
                U256::MAX,
                "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ),
        ] {
            assert_eq!(payload(&wide), payload(&hex));
            assert_eq!(postcard::from_bytes::<U256>(&payload(&hex)).unwrap(), wide);
        }
        assert_eq!(payload(&Date::EPOCH), payload(&0_i32));
        assert_eq!(payload(&Duration::from_millis(190)), payload(&190_u64));
        assert_eq!(payload(&Timestamp::from_millis(-190)), payload(&-190_i64));
        assert_eq!(
            payload(&Decimal::try_new(-125, 2).unwrap()),
            payload(&"-1.25")
        );
        assert_eq!(
            payload(&Float32::try_new(1.5).unwrap()),
            1.5_f32.to_le_bytes()
        );
        assert_eq!(
            payload(&Float64::try_new(-1.5).unwrap()),
            (-1.5_f64).to_le_bytes()
        );
    }
}

mod checked_text {
    use super::payload;
    use ic_db::design::prelude::*;

    #[newtype(
        item(prim = "Text", max_bytes = 200),
        validate(non_blank, max_chars = 50)
    )]
    struct CollectionName {}

    pub(super) fn check_payloads() {
        for text in [
            "  Game  ".to_owned(),
            "é".repeat(50),
            "🌱".repeat(50),
            "\0".to_owned(),
        ] {
            let name = CollectionName::try_from(text.clone()).unwrap();
            let bytes = payload(&text);
            assert_eq!(payload(&name), bytes);
            assert_eq!(
                postcard::from_bytes::<CollectionName>(&bytes).unwrap(),
                name
            );
            assert_eq!(String::from(name), text);
        }
        for text in [
            String::new(),
            "\u{3000}\u{a0}".into(),
            "é".repeat(51),
            "e\u{301}".repeat(26),
            "🌱".repeat(51),
        ] {
            assert!(CollectionName::try_from(text.clone()).is_err());
            assert!(postcard::from_bytes::<CollectionName>(&payload(&text)).is_err());
        }
    }
}
