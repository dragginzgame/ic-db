//! Downstream application payload and host inspection probe.
#![forbid(unsafe_code)]

use ic_db::design::prelude::*;
#[cfg(feature = "inspection")]
use ic_wire::typescript::Wire;
#[cfg(all(test, feature = "inspection"))]
use ic_wire::{Encoding, Limits, Message};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "inspection", derive(Wire))]
struct UserAdminRequest {
    user_id: Ulid,
    action: Action,
}

#[enum_(wire, variant(name = "Suspend"), variant(name = "Allow"))]
struct Action {}

#[newtype(wire, item(prim = "Nat16"))]
struct Code {}

#[list(wire, item(is = "Code"), max_items = 4)]
struct Codes {}

#[set(wire, item(prim = "Nat8"), max_items = 4)]
struct Roles {}

#[map(wire, key(prim = "Nat8"), value(item(prim = "Nat16")), max_items = 4)]
struct Scores {}

#[record(
    wire,
    fields(
        field(name = "scores", value(item(is = "Scores"))),
        field(name = "roles", value(item(is = "Roles"))),
        field(name = "codes", value(item(is = "Codes"))),
        field(name = "wide", value(item(prim = "U256")))
    )
)]
struct Profile {}

#[canister]
struct Directory {}

#[store(canister = Directory, key = "probe.profile.v1")]
struct Profiles {}

#[entity(wire, store = Profiles, pk(field = "id"), fields(
    field(name = "profile", value(item(is = "Profile"))),
    field(name = "id", value(item(prim = "Ulid")))
))]
struct User {}

#[cfg(all(test, feature = "inspection"))]
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
        #[cfg(all(test, feature = "inspection"))]
        {
            let message = ic_wire::encode(&request, Encoding::Payload, LIMITS).unwrap();
            assert_eq!(message, [expected.as_slice(), &[0]].concat());
            assert_eq!(
                ic_wire::decode::<UserAdminRequest>(&message, Encoding::Payload, LIMITS).unwrap(),
                request
            );
        }
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

#[cfg(all(test, feature = "inspection"))]
mod tests {
    use super::*;
    use ic_wire::typescript::{
        Method, PayloadLimits, SchemaLimits, SchemaRegistry, ValidatedRegistry,
    };

    #[derive(Serialize, Deserialize, Wire)]
    struct PageRequest {
        after: Option<ic_db::QueryCursor>,
    }

    #[derive(Serialize, Deserialize, Wire)]
    struct PageReply {
        id: u8,
        next: Option<ic_db::QueryCursor>,
    }

    #[test]
    fn checked_principal_uses_the_principal_payload_and_rejects_anonymous() {
        #[newtype(wire, item(prim = "Principal"), validate(non_anonymous))]
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
        let id = first.rows[0].id.to_bytes()[15];
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
            let encoded = ic_wire::encode(
                &Message(Some(ic_wire::ByteBuf::from(bad))),
                Encoding::Payload,
                limits,
            )
            .unwrap();
            assert!(
                ic_wire::decode::<Message<Option<ic_wire::ByteBuf>>>(
                    &encoded,
                    Encoding::Payload,
                    limits
                )
                .is_ok()
            );
            assert!(ic_wire::decode::<PageRequest>(&encoded, Encoding::Payload, limits).is_err());
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
        let manifests = registry.api_manifests(&[("page", method)]).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifests["page"]).unwrap();
        assert_eq!(manifest["request"]["root"], "PageRequest");
        assert_eq!(manifest["response"]["root"], "PageReply");
        let module = registry
            .method_module(&[method], "./runtime.ts", &Default::default(), |report| {
                assert!(report.is_clean(), "{report}");
            })
            .unwrap();
        let frame = |direction| Encoding::Frame {
            method: 1,
            direction,
        };
        let response = frame(ic_wire::Direction::Response);
        let request = frame(ic_wire::Direction::Request);
        assert_eq!(
            ic_wire::encode(&PageRequest { after: None }, request, LIMITS).unwrap(),
            ic_wire::encode(&Message(None::<Vec<u8>>), request, LIMITS).unwrap()
        );
        let byte_vector_reply = ic_wire::encode(
            &Message((id, Some(ic_wire::ByteBuf::from(cursor.to_bytes())))),
            response,
            LIMITS,
        )
        .unwrap();
        let first_reply = ic_wire::encode(
            &PageReply {
                id,
                next: Some(cursor.clone()),
            },
            response,
            LIMITS,
        )
        .unwrap();
        assert_eq!(first_reply, byte_vector_reply);
        assert_eq!(first_reply.last(), Some(&0));
        assert!(
            ic_wire::decode::<PageReply>(&first_reply[..first_reply.len() - 1], response, LIMITS)
                .is_err()
        );
        let next_request = typescript_continue(&module, &first_reply);
        assert_eq!(
            next_request,
            ic_wire::encode(
                &Message(Some(ic_wire::ByteBuf::from(cursor.to_bytes()))),
                request,
                LIMITS
            )
            .unwrap()
        );
        let decoded: PageRequest = ic_wire::decode(&next_request, request, LIMITS).unwrap();
        let second = query().after(&decoded.after.unwrap()).fetch().unwrap();
        assert_eq!(second.rows[0].id, Ulid::from_u128(2));
        assert_eq!(second.rows.len(), 1);
        assert!(ic_wire::decode::<PageRequest>(&[1, 50, 1], Encoding::Payload, LIMITS).is_err());
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
        let input = serde_json::json!({"module":module, "runtime":ic_wire::typescript::RUNTIME, "reply":reply});
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

    #[test]
    fn scalar_schema_matches_its_binary_representation() {
        fn layout<T: Wire + ?Sized>() -> String {
            ValidatedRegistry::from_type::<T>("Value", SchemaLimits::default())
                .unwrap()
                .metadata_json()
                .unwrap()
        }
        assert_eq!(
            layout::<ic_db::QueryCursor>(),
            layout::<ic_wire::Bytes<{ ic_db::QueryCursor::MAX_ENCODED_BYTES }>>()
        );
        assert_eq!(layout::<Code>(), layout::<u16>());
        assert_eq!(layout::<Date>(), layout::<i32>());
        assert_eq!(layout::<Timestamp>(), layout::<i64>());
        assert_eq!(layout::<Duration>(), layout::<u64>());
        assert_eq!(layout::<Decimal>(), layout::<str>());
        assert_eq!(layout::<U256>(), layout::<str>());
        assert_eq!(layout::<Float32>(), layout::<ic_wire::FiniteF32>());
        assert_eq!(layout::<Float64>(), layout::<ic_wire::FiniteF64>());
        assert_eq!(layout::<Blob>(), layout::<Vec<u8>>());
        assert_eq!(layout::<Subaccount>(), layout::<[u8; 32]>());
        let mut named = SchemaRegistry::new(SchemaLimits::default()).unwrap();
        named.register::<UserAdminRequest>("Request").unwrap();
        named.register::<User>("User").unwrap();
        named.register::<Account>("Account").unwrap();
        named.finish().unwrap();
    }

    #[test]
    fn inspection_uses_the_serde_field_order_and_enum_tags() {
        let registry =
            ValidatedRegistry::from_type::<UserAdminRequest>("Request", SchemaLimits::default())
                .unwrap();
        let limits = PayloadLimits {
            bytes: 2048,
            nodes: 256,
            depth: 16,
        };
        let method = Method {
            export: "userAdmin",
            id: 1,
            request: "Request",
            response: "Request",
            request_limits: limits,
            response_limits: limits,
        };
        let manifests = registry.api_manifests(&[("user_admin", method)]).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifests["user_admin"]).unwrap();
        let request: serde_json::Value =
            serde_json::from_str(&registry.metadata_json().unwrap()).unwrap();
        assert_eq!(
            request["Request"]["fields"][0],
            serde_json::json!({"name":"user_id","value":{"k":"array","max":16,"item":{"k":"u","bits":8}}})
        );
        let variants = &request["Request"]["fields"][1]["value"]["variants"];
        assert_eq!(variants[0]["name"], "Allow");
        assert_eq!(variants[0]["id"], 0);
        assert_eq!(variants[1]["id"], 1);
        assert_eq!(manifest["endpoint"], "user_admin");
        assert_eq!(manifest["schemas"]["Request"], request["Request"]);
        let registry =
            ValidatedRegistry::from_type::<User>("User", SchemaLimits::default()).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(&registry.metadata_json().unwrap()).unwrap();
        assert_eq!(metadata["User"]["k"], "record");
        assert_eq!(metadata["User"]["fields"][0]["value"]["k"], "record");
        let fields = &metadata["User"]["fields"];
        assert_eq!(fields[0]["name"], "profile");
        assert_eq!(fields[1]["name"], "id");
        let members = &fields[0]["value"]["fields"];
        assert_eq!(members[0]["name"], "codes");
        assert_eq!(
            members[1],
            serde_json::json!({"name":"roles","value":{"k":"list","item":{"k":"u","bits":8}}})
        );
        assert_eq!(
            members[2],
            serde_json::json!({"name":"scores","value":{"k":"list","item":{"k":"tuple","items":[{"k":"u","bits":8},{"k":"u","bits":16}]}}})
        );
    }
}

mod checked_text {
    use super::*;

    #[newtype(
        wire,
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
