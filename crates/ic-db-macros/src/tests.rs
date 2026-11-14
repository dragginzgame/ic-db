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
        quote!(id=1, pk(field="id"), fields(#field, field(name="title", value(item(prim="Text", max_bytes=0))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="title", value(item(prim="Text", max_bytes=4097))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="flag", value(item(prim="Bool", max_bytes=1))))),
        quote!(id=1, pk(field="id"), fields(#field, field(name="flag", value(opt, opt, item(prim="Bool"))))),
        quote!(id=1, pk(field="id"), index(id=1,field="title"), fields(#field, field(name="title", value(item(prim="Text",max_bytes=32))))),
        quote!(id=1, pk(field="id"), index(id=1,field="optional",unique), fields(#field, field(name="optional", value(opt,item(prim="Nat64"))))),
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
