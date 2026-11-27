//! Literal enum collection frames survive message rollback and upgrade.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::{Direction, Encoding};

#[tokio::test]
async fn enum_collections_preserve_bytes_and_atomicity() {
    let wasm = ic_db_m0_rollback_driver::records_wasm();
    let owned = instance().await.unwrap();
    let pic = &owned;
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 3_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    let call = async |mode: u8, bytes: Vec<u8>| {
        pic.update_call(
            canister,
            canister,
            "enum_collections",
            ic_wire::encode(
                &(mode, bytes),
                Encoding::Frame {
                    method: 94,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .unwrap(),
        )
        .await
    };
    // Reply order is Archived, Draft, Ready. Expected values below come
    // directly from the literal stored frames, independently of DB decoding.
    let membership = async || {
        let request = ic_wire::encode(
            &(),
            Encoding::Frame {
                method: 150,
                direction: Direction::Request,
            },
            LIMITS,
        )
        .unwrap();
        let reply = pic
            .query_call(canister, canister, "enum_set_membership", request)
            .await
            .unwrap();

        ic_wire::decode::<[bool; 3]>(
            &reply,
            Encoding::Frame {
                method: 150,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    let decode = |bytes: &[u8]| {
        ic_wire::decode::<Result<(Vec<u8>, u64), u8>>(
            bytes,
            Encoding::Frame {
                method: 94,
                direction: Direction::Response,
            },
            LIMITS,
        )
        .unwrap()
    };
    // counts: Draft/absent, Ready/256; id=1; states: Archived, Ready.
    // Draft appears only in the map. Set membership must still reject it.
    let golden = vec![
        2, 0, 0, 0, 1, 0, 2, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2,
    ];
    assert_eq!(
        decode(&call(1, golden.clone()).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    let before = pic.get_stable_memory(canister).await;
    assert_eq!(membership().await, [true, false, true]);
    assert_eq!(
        decode(&call(2, golden.clone()).await.unwrap()),
        Ok((golden.clone(), 1))
    );
    assert_eq!(pic.get_stable_memory(canister).await, before);
    for (offset, value) in [(6, 0), (4, 2), (6, 255), (23, 0), (22, 2), (23, 255)] {
        let mut bad = golden.clone();
        bad[offset] = value;
        assert_eq!(decode(&call(2, bad).await.unwrap()), Err(1));
        assert_eq!(pic.get_stable_memory(canister).await, before);
    }
    let empty = vec![0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(call(3, empty.clone()).await.is_err());
    assert_eq!(membership().await, [true, false, true]);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    ic_db_m0_rollback_driver::upgrade_stopped(pic, canister, wasm, vec![], None)
        .await
        .unwrap();
    assert_eq!(decode(&call(0, vec![]).await.unwrap()), Ok((golden, 1)));
    assert_eq!(membership().await, [true, false, true]);
    assert_eq!(pic.get_stable_memory(canister).await, before);
    assert_eq!(
        decode(&call(2, empty.clone()).await.unwrap()),
        Ok((empty, 2))
    );
    assert_eq!(membership().await, [false; 3]);
    assert_eq!(decode(&call(4, vec![]).await.unwrap()), Ok((vec![], 3)));
    assert_eq!(membership().await, [false; 3]);
    owned.drop().await;
}
