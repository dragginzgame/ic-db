use super::expand;
use quote::quote;

#[test]
fn invalid_authoring_refuses_before_emitting_a_schema() {
    let field = quote!(field(name = "id", value(item(prim = "Nat64"))));
    for args in [
        quote!(id=1, pk(field="missing"), fields(#field)),
        quote!(id=1, pk(field="id"), fields(#field, #field)),
        quote!(id=1, id=2, pk(field="id"), fields(#field)),
        quote!(id=1, pk(field="id"), index(id=1, fields=["missing"]), fields(#field)),
        quote!(
            id = 1,
            pk(field = "id"),
            fields(field(name = "id", id = 2, value(item(prim = "Nat64"))))
        ),
        quote!(
            id = 1,
            pk(field = "id"),
            fields(field(name = "id", value(item(prim = "Text"))))
        ),
        quote!(id=1, pk(field="id", source="generated"), fields(#field)),
        quote!(
            id = 1,
            pk(field = "id"),
            fields(field(name = "id", value(opt, item(prim = "Nat64"))))
        ),
        quote!(id=1, pk(field="id"), fields(#field, field(name="title", value(item(prim="Text", max_bytes=4097))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="flag", value(item(prim="Bool", max_bytes=1))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="flag", value(opt, opt, item(prim="Bool"))))),
        quote!(id=1, pk(field="id"), index(id=1,field="title"), fields(#field, field(name="title", value(item(prim="Text",max_bytes=32))))),
        quote!(id=1, pk(field="id"), index(id=1,field="optional",unique), fields(#field, field(name="optional", value(opt,item(prim="Nat64"))))),
        quote!(
            id = 1,
            pk(field = "id"),
            fields(field(name = "id", value(item(prim = "Int64"))))
        ),
        quote!(id=1, pk(field="id"), index(id=1,field="delta"), fields(#field, field(name="delta",value(item(prim="Int64"))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="delta",value(item(prim="Int64",max_bytes=8))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="payload",value(item(prim="Blob"))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="payload",value(item(prim="Blob",max_bytes=4097))))),
        quote!(id=1, pk(field="id"), index(id=1,field="payload"), fields(#field, field(name="payload",value(item(prim="Blob",max_bytes=8))))),
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
    for args in [
        quote!(
            id = 1,
            pk(field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = 2, index = 1)
            ))
        ),
        quote!(
            id = 1,
            pk(field = "id"),
            index(id = 1, field = "id"),
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(
                    name = "target",
                    value(opt, item(prim = "Nat64")),
                    references(entity = 2, index = 1)
                )
            )
        ),
        quote!(
            id = 1,
            pk(field = "id"),
            index(id = 1, field = "id"),
            fields(
                field(name = "id", value(item(prim = "Nat64"))),
                field(
                    name = "target",
                    value(item(prim = "Text", max_bytes = 8)),
                    references(entity = 2, index = 1)
                )
            )
        ),
        quote!(
            id = 1,
            pk(field = "id"),
            index(id = 1, field = "id"),
            fields(field(
                name = "id",
                value(item(prim = "Nat64")),
                references(entity = 0, index = 1)
            ))
        ),
        quote!(
            id = 1,
            pk(field = "id"),
            index(id = 1, field = "id"),
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
fn integer_storage_does_not_admit_unimplemented_keys_or_bounds() {
    for prim in [
        "Nat8", "Nat16", "Nat32", "Int8", "Int16", "Int32", "Nat128", "Int128",
    ] {
        let id = quote!(field(name = "id", value(item(prim = "Nat64"))));
        let wide = quote!(field(name="wide",value(item(prim=#prim))));
        for args in [
            quote!(id=1,pk(field="wide"),fields(#wide)),
            quote!(id=1,pk(field="id"),index(id=1,field="wide"),fields(#id,#wide)),
            quote!(id=1,pk(field="id"),fields(#id,field(name="wide",value(item(prim=#prim,max_bytes=16))))),
            quote!(id=1,pk(field="id"),index(id=1,field="wide"),fields(#id,field(name="wide",value(item(prim=#prim)),references(entity=1,index=1)))),
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
