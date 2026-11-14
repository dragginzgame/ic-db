use super::expand;
use quote::quote;

#[test]
fn invalid_authoring_refuses_before_emitting_a_schema() {
    let field = quote!(field(name = "id", value(item(prim = "Nat64"))));
    for args in [
        quote!(pk(field="missing"), fields(#field)),
        quote!(pk(field="id"), fields(#field, #field)),
        quote!(id=2, pk(field="id"), fields(#field)),
        quote!(pk(field="id"), index(fields=["missing"]), fields(#field)),
        quote!(
            pk(field = "id"),
            fields(field(name = "id", id = 2, value(item(prim = "Nat64"))))
        ),
        quote!(
            pk(field = "id"),
            fields(field(name = "id", value(item(prim = "Text"))))
        ),
        quote!(pk(field="id", source="generated"), fields(#field)),
        quote!(
            pk(field = "id"),
            fields(field(name = "id", value(opt, item(prim = "Nat64"))))
        ),
        quote!(pk(field="id"), fields(#field, field(name="title", value(item(prim="Text", max_bytes=4097))))),
        quote!(pk(field="id"), fields(#field, field(name="flag", value(item(prim="Bool", max_bytes=1))))),
        quote!(pk(field="id"), fields(#field, field(name="flag", value(opt, opt, item(prim="Bool"))))),
        quote!(pk(field="id"), index(field="title",unique), fields(#field, field(name="title", value(opt,item(prim="Text",max_bytes=32))))),
        quote!(pk(field="id"), index(field="optional",unique), fields(#field, field(name="optional", value(opt,item(prim="Nat64"))))),
        quote!(
            pk(field = "id"),
            fields(field(name = "id", value(item(prim = "Float64"))))
        ),
        quote!(pk(field="id"), fields(#field, field(name="delta",value(item(prim="Int64",max_bytes=8))))),
        quote!(pk(field="id"), fields(#field, field(name="payload",value(item(prim="Blob"))))),
        quote!(pk(field="id"), fields(#field, field(name="payload",value(item(prim="Blob",max_bytes=4097))))),
        quote!(pk(field="id"), index(field="payload"), fields(#field, field(name="payload",value(item(prim="Blob",max_bytes=8))))),
    ] {
        assert!(
            expand(
                args,
                quote!(
                    pub struct Entry {}
                )
            )
            .is_err()
        );
    }
}

#[test]
fn references_require_a_bounded_reverse_access_path() {
    let optional = quote!(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "target",
            value(opt, item(prim = "Nat64")),
            references(entity = "Target")
        )
    );
    assert!(
        expand(
            quote!(pk(field="id"), index(fields=["target","id"]), fields(#optional)),
            quote!(
                struct Row {}
            )
        )
        .is_ok()
    );
    assert!(
        expand(
            quote!(pk(field="id"), index(field="target",unique,present_only), fields(#optional)),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    for args in [
        quote!(
            pk(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = "Target")
            ))
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(
                    name = "target",
                    value(opt, item(prim = "Nat64")),
                    references(entity = "Target")
                )
            )
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(
                    name = "target",
                    value(item(prim = "Text", max_bytes = 8)),
                    references(entity = "Target")
                )
            )
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = 0, index = 1)
            ))
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = 2)
            ))
        ),
    ] {
        assert!(
            expand(
                args,
                quote!(
                    struct Row {}
                )
            )
            .is_err()
        );
    }
}

#[test]
fn scalar_storage_does_not_admit_unimplemented_keys_or_bounds() {
    for prim in [
        "Date",
        "Timestamp",
        "Duration",
        "Account",
        "Subaccount",
        "Float32",
        "Float64",
        "Principal",
    ] {
        let id = quote!(field(name = "id", value(item(prim = "Nat64"))));
        let wide = quote!(field(name="wide",value(item(prim=#prim))));
        for args in [
            quote!(pk(field="wide"),fields(#wide)),
            quote!(pk(field="id"),index(field="wide",unique),fields(#id,field(name="wide",value(opt,item(prim=#prim))))),
            quote!(pk(field="id"),fields(#id,field(name="wide",value(item(prim=#prim,max_bytes=16))))),
            quote!(pk(field="id"),index(field="wide"),fields(#id,field(name="wide",value(item(prim=#prim)),references(entity = "Self")))),
        ] {
            assert!(
                expand(
                    args,
                    quote!(
                        struct Row {}
                    )
                )
                .is_err()
            );
        }
    }
}

#[test]
fn optional_membership_is_explicit_and_cannot_hide_reverse_references() {
    let fields = quote!(fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(
            name = "alias",
            value(opt, item(prim = "Text", max_bytes = 32))
        ),
        field(name = "code", value(opt, item(prim = "Int64")))
    ));
    for index in [
        quote!(index(fields = ["alias", "code"], unique)),
        quote!(index(field = "alias", present_only)),
        quote!(index(field = "id", unique, present_only)),
        quote!(index(field = "alias", unique, present_only, present_only)),
    ] {
        assert!(
            expand(
                quote!(pk(field="id"),#index,#fields),
                quote!(
                    struct Row {}
                )
            )
            .is_err()
        );
    }
    for index in [
        quote!(index(fields = ["alias", "code"], unique, present_only)),
        quote!(index(fields = ["alias", "code"])),
    ] {
        assert!(
            expand(
                quote!(pk(field="id"),#index,#fields),
                quote!(
                    struct Row {}
                )
            )
            .is_ok()
        );
    }
    assert!(
        expand(
            quote!(
                pk(field = "id"),
                index(fields = ["id", "alias"], unique, present_only),
                fields(
                    field(
                        name = "id",
                        value(item(prim = "Nat64")),
                        references(entity = "Self")
                    ),
                    field(
                        name = "alias",
                        value(opt, item(prim = "Text", max_bytes = 32))
                    )
                )
            ),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
}

#[test]
fn names_and_reverse_selection_are_unambiguous() {
    let field = quote!(field(name = "id", value(item(prim = "Nat64"))));
    let reference = quote!(fields(#field,field(name="target",value(item(prim="Nat64")),references(entity="Target"))));
    for args in [
        quote!(id=1, pk(field="id"), fields(#field)),
        quote!(pk(field="id"), index(id=1,field="id"), fields(#field)),
        quote!(name="", pk(field="id"), fields(#field)),
        quote!(pk(field="id"), index(name="",field="id"), fields(#field)),
        quote!(pk(field="id"), index(field="id"), index(field="id",unique), fields(#field)),
        quote!(pk(field="id"), index(name="same",field="id"), index(name="same",field="id",unique), fields(#field)),
        quote!(pk(field="id"), #reference),
        quote!(pk(field="id"), index(field="target"), index(fields=["target","id"]), #reference),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = 1)
            ))
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = "Self", index = 1)
            ))
        ),
        quote!(
            pk(field = "id"),
            index(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = "Self", index = "missing")
            ))
        ),
    ] {
        assert!(
            expand(
                args.clone(),
                quote!(
                    struct Row {}
                )
            )
            .is_err(),
            "{args}"
        );
    }
    for args in [
        quote!(pk(field="id"), index(field="target"), #reference),
        quote!(pk(field="id"), index(field="target"), index(name="by_target",fields=["target","id"]), fields(#field,field(name="target",value(item(prim="Nat64")),references(entity="Target",index="by_target")))),
        quote!(name="Stable",pk(field="id"), index(field="id"), index(name="unique_id",field="id",unique), fields(#field)),
        quote!(pk(field="id"), index(field="a_b"), index(fields=["a","b"]), fields(#field,field(name="a_b",value(item(prim="Nat64"))),field(name="a",value(item(prim="Nat64"))),field(name="b",value(item(prim="Nat64"))))),
    ] {
        assert!(
            expand(
                args.clone(),
                quote!(
                    struct Row {}
                )
            )
            .is_ok(),
            "{args}"
        );
    }
    let long = "x".repeat(65);
    assert!(
        expand(
            quote!(name=#long,pk(field="id"),fields(#field)),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    assert!(
        expand(
            quote!(pk(field="id"),index(name=#long,field="id"),fields(#field)),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
}

#[test]
fn enum_authoring_is_bounded_and_does_not_imply_index_support() {
    for args in [
        quote!(),
        quote!(variant(name = "A"), variant(name = "A")),
        quote!(variant(name = "")),
        quote!(variant(name = "A", references(entity = "Target"))),
        quote!(id = 1, variant(name = "A")),
    ] {
        assert!(
            super::enumeration::expand(
                args,
                quote!(
                    struct State {}
                )
            )
            .is_err()
        );
    }
    let variants: Vec<_> = (0..65)
        .map(|i| {
            let name = format!("V{i}");
            quote!(variant(name=#name))
        })
        .collect();
    assert!(
        super::enumeration::expand(
            quote!(#(#variants),*),
            quote!(
                struct State {}
            )
        )
        .is_err()
    );
    let fields = quote!(fields(
        field(name = "id", value(item(prim = "Nat64"))),
        field(name = "state", value(item(is = "State")))
    ));
    assert!(
        expand(
            quote!(pk(field="id"),#fields),
            quote!(
                struct Row {}
            )
        )
        .is_ok()
    );
    assert!(
        expand(
            quote!(pk(field="id"),index(field="state"),#fields),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    assert!(
        expand(
            quote!(pk(field="state"),#fields),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    assert!(
        expand(
            quote!(
                pk(field = "id"),
                fields(
                    field(name = "id", value(item(prim = "Nat64"))),
                    field(name = "state", value(item(is = "State", prim = "Nat8")))
                )
            ),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
}

#[test]
fn nested_authoring_preserves_one_field_rule_owner() {
    for args in [
        quote!(fields(
            field(name = "x", value(item(prim = "Nat64"))),
            field(name = "x", value(item(prim = "Nat64")))
        )),
        quote!(fields(field(
            name = "x",
            value(item(prim = "Nat64")),
            references(entity = "Target")
        ))),
        quote!(pk(field = "x"), fields()),
        quote!(fields(field(
            name = "x",
            value(item(indirect, prim = "Nat64"))
        ))),
    ] {
        assert!(
            super::record::expand(
                args,
                quote!(
                    struct Nested {}
                )
            )
            .is_err()
        );
    }
    assert!(
        super::record::expand(
            quote!(fields(field(
                name = "next",
                value(opt, item(indirect, is = "Self"))
            ))),
            quote!(
                struct Node {}
            )
        )
        .is_ok()
    );
    assert!(
        super::enumeration::expand(
            quote!(
                variant(name = "End"),
                variant(name = "Next", value(item(indirect, is = "Self")))
            ),
            quote!(
                struct Node {}
            )
        )
        .is_ok()
    );
}

#[test]
fn list_authoring_requires_explicit_bounds_and_refuses_unsupported_key_shapes() {
    let id = quote!(field(name = "id", value(item(prim = "Nat64"))));
    for value in [
        quote!(value(many, item(prim = "Nat8"))),
        quote!(value(max_items = 1, item(prim = "Nat8"))),
        quote!(value(many, max_items = 257, item(prim = "Nat8"))),
        quote!(value(many, many, max_items = 1, item(prim = "Nat8"))),
    ] {
        assert!(
            expand(
                quote!(pk(field="id"),fields(#id,field(name="items",#value))),
                quote!(
                    struct Row {}
                )
            )
            .is_err()
        );
    }
    let items = quote!(field(
        name = "items",
        value(many, max_items = 4, item(prim = "Nat64"))
    ));
    assert!(
        expand(
            quote!(pk(field="id"),fields(#id,#items)),
            quote!(
                struct Row {}
            )
        )
        .is_ok()
    );
    assert!(
        expand(
            quote!(pk(field="items"),fields(#id,#items)),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    assert!(
        expand(
            quote!(pk(field="id"),index(field="items"),fields(#id,#items)),
            quote!(
                struct Row {}
            )
        )
        .is_err()
    );
    assert!(expand(quote!(pk(field="id"),index(field="items"),fields(#id,field(name="items",value(many,max_items=4,item(prim="Nat64")),references(entity="Target")))),quote!(struct Row{})).is_err());
}

#[test]
fn named_sets_require_bounded_ordered_scalar_elements() {
    let declaration = quote!(
        struct Scores {}
    );
    assert!(
        super::set::expand(
            quote!(item(prim = "Int16"), max_items = 3),
            declaration.clone()
        )
        .is_ok()
    );
    for args in [
        quote!(item(prim = "Int16")),
        quote!(item(prim = "Int16"), max_items = 257),
        quote!(item(is = "Other"), max_items = 3),
        quote!(item(prim = "Text"), max_items = 3),
        quote!(item(prim = "Int16"), max_items = 3, opt),
        quote!(item(prim = "Int16"), max_items = 3, max_items = 2),
    ] {
        assert!(super::set::expand(args, declaration.clone()).is_err());
    }
}

#[test]
fn maps_require_scalar_keys_bounded_counts_and_an_explicit_value() {
    let declaration = quote!(
        struct Rates {}
    );
    assert!(
        super::map::expand(
            quote!(
                key(prim = "Int16"),
                value(opt, many, max_items = 3, item(is = "Self")),
                max_items = 3
            ),
            declaration.clone()
        )
        .is_ok()
    );
    for args in [
        quote!(key(prim = "Int16"), value(item(prim = "Nat8"))),
        quote!(key(prim = "Int16"), max_items = 3),
        quote!(key(is = "Other"), value(item(prim = "Nat8")), max_items = 3),
        quote!(
            key(prim = "Text"),
            value(item(prim = "Nat8")),
            max_items = 3
        ),
        quote!(
            key(prim = "Int16", opt),
            value(item(prim = "Nat8")),
            max_items = 3
        ),
        quote!(
            key(prim = "Nat8"),
            value(item(prim = "Bool")),
            max_items = 257
        ),
        quote!(
            key(prim = "Nat8"),
            value(item(prim = "Bool")),
            max_items = 3,
            max_items = 2
        ),
    ] {
        assert!(super::map::expand(args, declaration.clone()).is_err());
    }
}

#[test]
fn decimal_requires_only_its_bounded_declared_scale() {
    for item in [
        quote!(prim = "Decimal"),
        quote!(prim = "Decimal", scale = 29),
        quote!(prim = "Decimal", scale = 2, scale = 2),
        quote!(prim = "Decimal", scale = 2, max_bytes = 17),
        quote!(prim = "Int128", scale = 2),
        quote!(is = "Nested", scale = 2),
    ] {
        assert!(expand(quote!(pk(field="id"),fields(field(name="id",value(item(prim="Nat64"))),field(name="value",value(item(#item))))),quote!(struct Row {})).is_err());
    }
    for scale in [0_u32, 2, 28] {
        assert!(expand(quote!(pk(field="id"),index(field="value"),fields(field(name="id",value(item(prim="Nat64"))),field(name="value",value(item(prim="Decimal",scale=#scale))))),quote!(struct Row {})).is_ok());
    }
}

#[test]
fn ulid_keys_are_supplied_nonoptional_scalars() {
    let id = quote!(field(name = "id", value(item(prim = "Ulid"))));
    assert!(expand(quote!(pk(field="id"),index(field="next"),fields(#id,field(name="next",value(opt,item(prim="Ulid")),references(entity="Self")))),quote!(struct Row {})).is_ok());
    for args in [
        quote!(
            pk(field = "id"),
            fields(field(name = "id", value(opt, item(prim = "Ulid"))))
        ),
        quote!(
            pk(field = "id"),
            fields(field(
                name = "id",
                value(many, max_items = 2, item(prim = "Ulid"))
            ))
        ),
        quote!(pk(field="id",source="generated"),fields(#id)),
        quote!(pk(field="id"),fields(#id,field(name="next",value(item(prim="Ulid",max_bytes=16))))),
    ] {
        assert!(
            expand(
                args,
                quote!(
                    struct Row {}
                )
            )
            .is_err()
        );
    }
}

#[test]
fn integer_keys_require_nonoptional_scalar_fields() {
    for prim in [
        "Nat8", "Nat16", "Nat32", "Nat64", "Nat128", "Int8", "Int16", "Int32", "Int64", "Int128",
        "U256",
    ] {
        let id = quote!(field(name="id",value(item(prim=#prim))));
        assert!(
            expand(
                quote!(pk(field="id"),fields(#id)),
                quote!(
                    struct Row {}
                )
            )
            .is_ok()
        );
        for args in [
            quote!(pk(field="id"),fields(field(name="id",value(opt,item(prim=#prim))))),
            quote!(pk(field="id"),fields(field(name="id",value(many,max_items=2,item(prim=#prim))))),
            quote!(pk(field="id"),fields(field(name="id",value(item(prim=#prim,max_bytes=32))))),
            quote!(pk(field="id",source="generated"),fields(#id)),
        ] {
            assert!(
                expand(
                    args,
                    quote!(
                        struct Row {}
                    )
                )
                .is_err()
            );
        }
    }
}
