use crate::helpers::*;
use async_graphql_parser::types::{
    BaseType, FieldDefinition, Type, TypeDefinition, TypeKind,
};
use async_graphql_parser::{Pos, Positioned};
use async_graphql_value::Name;
use fuel_indexer_lib::{
    graphql::{types::IdCol, GraphQLSchemaValidator, ParsedGraphQLSchema},
    type_id, ExecutionSource,
};
use linked_hash_set::LinkedHashSet;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, HashSet};
use syn::Ident;

/// `Decoder`s are responsible for transforming GraphQL `TypeDefinition`s into
/// token streams that can be used to generate Rust code for indexing types.
pub trait Decoder {
    /// Create a decoder from a GraphQL `TypeDefinition`.
    fn from_typedef(typ: &TypeDefinition, parsed: &ParsedGraphQLSchema) -> Self;
}

/// A wrapper object used to process GraphQL `TypeKind::Object` type definitions
/// into a format from which Rust tokens can be generated.
pub struct ObjectDecoder {
    /// The name of the GraphQL object (as a `syn::Ident`).
    ident: Ident,

    /// Tokens used to create fields in the struct definition.
    struct_fields: TokenStream,

    /// Tokens used to extract each individual field from a row.
    field_extractors: TokenStream,

    /// Tokens used to create fields in the `Entity::from_row` function.
    from_row: TokenStream,

    /// Tokens used to create fields in the `Entity::to_row` function.
    to_row: TokenStream,

    /// Tokens for the parameters of the `Entity::new` function.
    impl_new_params: ImplNewParameters,

    /// The source of the GraphQL schema.
    exec_source: ExecutionSource,

    /// The unique ID of this GraphQL type.
    type_id: i64,
}

impl Default for ObjectDecoder {
    fn default() -> Self {
        Self {
            ident: format_ident!("ObjectDecoder"),
            struct_fields: quote! {},
            field_extractors: quote! {},
            from_row: quote! {},
            to_row: quote! {},
            exec_source: ExecutionSource::Wasm,
            impl_new_params: ImplNewParameters::ObjectType {
                strct: format_ident!("ObjectDecoder"),
                parameters: quote! {},
                hasher: quote! {},
                object_name: "".to_string(),
                struct_fields: quote! {},
                exec_source: ExecutionSource::Wasm,
                field_set: HashSet::new(),
            },
            type_id: std::i64::MAX,
        }
    }
}

impl Decoder for ObjectDecoder {
    /// Create a decoder from a GraphQL `TypeKind::Object`.
    fn from_typedef(typ: &TypeDefinition, parsed: &ParsedGraphQLSchema) -> Self {
        match &typ.kind {
            TypeKind::Object(o) => {
                let obj_name = typ.name.to_string();
                let ident = format_ident!("{}", obj_name);
                let type_id = type_id(&parsed.fully_qualified_namespace(), &obj_name);

                let mut struct_fields = quote! {};
                let mut field_extractors = quote! {};
                let mut from_row = quote! {};
                let mut to_row = quote! {};
                let mut parameters = quote! {};
                let mut hasher = quote! { Sha256::new() };
                let mut impl_new_fields = quote! {};

                let mut fields_map = BTreeMap::new();
                let obj_field_names = parsed
                    .object_field_mappings
                    .get(&obj_name)
                    .unwrap_or_else(|| {
                        panic!("TypeDefinition '{obj_name}' not found in parsed schema.")
                    })
                    .iter()
                    .map(|(k, _v)| k.to_owned())
                    .collect::<HashSet<String>>();
                GraphQLSchemaValidator::check_disallowed_graphql_typedef_name(&obj_name);

                for field in &o.fields {
                    let (field_typ_tokens, field_name, field_typ_scalar_name, extractor) =
                        process_typedef_field(parsed, field.node.clone());

                    let field_typ_scalar_name = &field_typ_scalar_name.to_string();

                    fields_map
                        .insert(field_name.to_string(), field_typ_scalar_name.clone());

                    let clone = clone_tokens(field_typ_scalar_name);
                    let field_decoder = field_decoder_tokens(
                        field.node.ty.node.nullable,
                        field_typ_scalar_name,
                        &field_name,
                        clone.clone(),
                    );

                    struct_fields = quote! {
                        #struct_fields
                        #field_name: #field_typ_tokens,
                    };

                    field_extractors = quote! {
                        #extractor
                        #field_extractors
                    };

                    from_row = quote! {
                        #from_row
                        #field_name,
                    };

                    to_row = quote! {
                        #to_row
                        #field_decoder
                    };

                    let unwrap_or_default = unwrap_or_default_tokens(
                        field_typ_scalar_name,
                        field.node.ty.node.nullable,
                    );
                    let to_bytes = to_bytes_tokens(field_typ_scalar_name);

                    if can_derive_id(&obj_field_names, &field_name.to_string(), &obj_name)
                    {
                        parameters =
                            parameters_tokens(parameters, &field_name, field_typ_tokens);
                        if let Some(tokens) = hasher_tokens(
                            field_typ_scalar_name,
                            hasher.clone(),
                            &field_name,
                            clone,
                            unwrap_or_default,
                            to_bytes,
                        ) {
                            hasher = tokens;
                        }

                        impl_new_fields = quote! {
                            #impl_new_fields
                            #field_name,
                        };
                    }
                }

                Self {
                    ident: ident.clone(),
                    struct_fields,
                    field_extractors,
                    from_row,
                    to_row,
                    exec_source: parsed.exec_source().clone(),
                    impl_new_params: ImplNewParameters::ObjectType {
                        // standardize all these names
                        strct: ident,
                        parameters,
                        hasher,
                        object_name: obj_name,
                        struct_fields: impl_new_fields,
                        exec_source: parsed.exec_source().clone(),
                        field_set: obj_field_names,
                    },
                    type_id,
                }
            }
            TypeKind::Union(u) => {
                // TODO: https://github.com/FuelLabs/fuel-indexer/issues/1031
                let union_name = typ.name.to_string();
                let ident = format_ident!("{}", union_name);
                let type_id = type_id(&parsed.fully_qualified_namespace(), &union_name);

                let mut struct_fields = quote! {};
                let mut field_extractors = quote! {};
                let mut from_row = quote! {};
                let mut to_row = quote! {};
                let mut parameters = quote! {};
                let mut hasher = quote! { Sha256::new() };
                let mut impl_new_fields = quote! {};

                let member_fields =
                    u.members
                        .iter()
                        .flat_map(|m| {
                            let name = m.node.to_string();
                            parsed
                        .object_field_mappings
                        .get(&name)
                        .unwrap_or_else(|| {
                            panic!("Could not find union member '{name}' in the schema.",)
                        })
                        .iter()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        })
                        .collect::<LinkedHashSet<(String, String)>>();

                let mut derived_type_fields = HashSet::new();
                let mut union_field_set = HashSet::new();

                let obj_field_names = member_fields
                    .iter()
                    .map(|(k, _v)| k.to_owned())
                    .collect::<HashSet<String>>();

                for (field_name, field_typ_name) in member_fields.iter() {
                    GraphQLSchemaValidator::derived_field_type_is_consistent(
                        &union_name,
                        field_name,
                        &derived_type_fields,
                    );
                    derived_type_fields.insert(field_name.to_owned());

                    let field = FieldDefinition {
                        description: None,
                        name: Positioned::new(Name::new(field_name), Pos::default()),
                        arguments: Vec::new(),
                        ty: Positioned::new(
                            Type {
                                base: BaseType::Named(Name::new(field_typ_name)),
                                nullable: field_typ_name != IdCol::to_uppercase_str(),
                            },
                            Pos::default(),
                        ),
                        directives: Vec::new(),
                    };

                    union_field_set.insert(field_name.clone());

                    // Since we've already processed the member's fields, we don't need
                    // to do any type of special field processing here.
                    let (field_typ_tokens, field_name, field_typ_scalar_name, extractor) =
                        process_typedef_field(parsed, field.clone());

                    let field_typ_scalar_name = &field_typ_scalar_name.to_string();

                    let clone = clone_tokens(field_typ_scalar_name);
                    let field_decoder = field_decoder_tokens(
                        field.ty.node.nullable,
                        field_typ_scalar_name,
                        &field_name,
                        clone.clone(),
                    );

                    struct_fields = quote! {
                        #struct_fields
                        #field_name: #field_typ_tokens,
                    };

                    field_extractors = quote! {
                        #extractor
                        #field_extractors
                    };

                    from_row = quote! {
                        #from_row
                        #field_name,
                    };

                    to_row = quote! {
                        #to_row
                        #field_decoder
                    };

                    let unwrap_or_default = unwrap_or_default_tokens(
                        field_typ_scalar_name,
                        field.ty.node.nullable,
                    );
                    let to_bytes = to_bytes_tokens(field_typ_scalar_name);

                    if can_derive_id(
                        &obj_field_names,
                        &field_name.to_string(),
                        &union_name,
                    ) {
                        parameters =
                            parameters_tokens(parameters, &field_name, field_typ_tokens);
                        if let Some(tokens) = hasher_tokens(
                            field_typ_scalar_name,
                            hasher.clone(),
                            &field_name,
                            clone,
                            unwrap_or_default,
                            to_bytes,
                        ) {
                            hasher = tokens;
                        }

                        impl_new_fields = quote! {
                            #impl_new_fields
                            #field_name,
                        };
                    }
                }

                Self {
                    ident: ident.clone(),
                    type_id,
                    struct_fields,
                    field_extractors,
                    from_row,
                    to_row,
                    exec_source: parsed.exec_source().clone(),
                    impl_new_params: ImplNewParameters::UnionType {
                        schema: parsed.clone(),
                        union_obj: u.clone(),
                        union_ident: ident,
                        union_field_set: obj_field_names,
                    },
                }
            }
            _ => panic!("Expected `TypeKind::Union` or `TypeKind::Object."),
        }
    }
}

/// A wrapper object used to process GraphQL `TypeKind::Enum` type definitions
/// into a format from which Rust tokens can be generated.
pub struct EnumDecoder {
    /// The name of the GraphQL enum (as a `syn::Ident`).
    ident: Ident,

    /// Tokens used to create fields in the `From<String> for #ident` function.
    to_enum: Vec<proc_macro2::TokenStream>,

    /// Tokens used to create fields in the `From<#ident> for String` function.
    from_enum: Vec<proc_macro2::TokenStream>,

    /// Tokens used to create fields in the enum definition.
    values: Vec<TokenStream>,

    /// The unique ID of this GraphQL type.
    ///
    /// Type IDs for enum types are only for reference since an enum is a virtual type.
    #[allow(unused)]
    type_id: i64,
}

impl Decoder for EnumDecoder {
    /// Create a decoder from a GraphQL `TypeKind::Enum`.
    fn from_typedef(typ: &TypeDefinition, parsed: &ParsedGraphQLSchema) -> Self {
        match &typ.kind {
            TypeKind::Enum(e) => {
                let enum_name = typ.name.to_string();
                let ident = format_ident!("{}", enum_name);
                let type_id = type_id(&parsed.fully_qualified_namespace(), &enum_name);

                let values = e
                    .values
                    .iter()
                    .map(|v| {
                        let ident = format_ident! {"{}", v.node.value.to_string()};
                        quote! { #ident }
                    })
                    .collect::<Vec<TokenStream>>();

                let to_enum = e
                    .values
                    .iter()
                    .map(|v| {
                        let value_ident = format_ident! {"{}", v.node.value.to_string()};
                        let as_str = format!("{}::{}", ident, value_ident);
                        quote! { #as_str => #ident::#value_ident, }
                    })
                    .collect::<Vec<proc_macro2::TokenStream>>();

                let from_enum = e
                    .values
                    .iter()
                    .map(|v| {
                        let value_ident = format_ident! {"{}", v.node.value.to_string()};
                        let as_str = format!("{}::{}", ident, value_ident);
                        quote! { #ident::#value_ident => #as_str.to_string(), }
                    })
                    .collect::<Vec<proc_macro2::TokenStream>>();

                Self {
                    ident,
                    to_enum,
                    from_enum,
                    values,
                    type_id,
                }
            }
            _ => panic!("Expected `TypeKind::Enum`."),
        }
    }
}

impl From<ObjectDecoder> for TokenStream {
    fn from(decoder: ObjectDecoder) -> Self {
        let ObjectDecoder {
            struct_fields,
            ident,
            field_extractors,
            from_row,
            to_row,
            impl_new_params,
            exec_source,
            type_id,
            ..
        } = decoder;

        let impl_json = quote! {

            impl From<#ident> for Json {
                fn from(value: #ident) -> Self {
                    let s = serde_json::to_string(&value).expect("Serde error.");
                    Self(s)
                }
            }

            impl From<Json> for #ident {
                fn from(value: Json) -> Self {
                    let s: #ident = serde_json::from_str(&value.0).expect("Serde error.");
                    s
                }
            }
        };

        let impl_entity = match exec_source {
            ExecutionSource::Native => quote! {
                #[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
                pub struct #ident {
                    #struct_fields
                }

                #[async_trait::async_trait]
                impl Entity for #ident {
                    const TYPE_ID: i64 = #type_id;

                    fn from_row(mut vec: Vec<FtColumn>) -> Self {
                        #field_extractors
                        Self {
                            #from_row
                        }
                    }

                    fn to_row(&self) -> Vec<FtColumn> {
                        vec![
                            #to_row
                        ]
                    }

                    async fn load(id: u64) -> Option<Self> {
                        unsafe {
                            match &db {
                                Some(d) => {
                                    match d.lock().await.get_object(Self::TYPE_ID, id).await {
                                        Some(bytes) => {
                                            let columns: Vec<FtColumn> = bincode::deserialize(&bytes).expect("Serde error.");
                                            let obj = Self::from_row(columns);
                                            Some(obj)
                                        },
                                        None => None,
                                    }
                                }
                                None => None,
                            }
                        }
                    }

                    async fn save(&self) {
                        unsafe {
                            match &db {
                                Some(d) => {
                                    d.lock().await.put_object(
                                        Self::TYPE_ID,
                                        self.to_row(),
                                        serialize(&self.to_row())
                                    ).await;
                                }
                                None => {},
                            }
                        }
                    }
                }
            },
            ExecutionSource::Wasm => quote! {
                #[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
                pub struct #ident {
                    #struct_fields
                }

                impl Entity for #ident {
                    const TYPE_ID: i64 = #type_id;

                    fn from_row(mut vec: Vec<FtColumn>) -> Self {
                        #field_extractors
                        Self {
                            #from_row
                        }
                    }

                    fn to_row(&self) -> Vec<FtColumn> {
                        vec![
                            #to_row
                        ]
                    }

                }

            },
        };

        let impl_new = match impl_new_params {
            ImplNewParameters::ObjectType {
                strct,
                parameters,
                hasher,
                object_name,
                struct_fields,
                exec_source,
                field_set,
            } => {
                if field_set.contains(&IdCol::to_lowercase_string()) {
                    generate_struct_new_method_impl(
                        strct,
                        parameters,
                        hasher,
                        object_name,
                        struct_fields,
                        exec_source,
                    )
                } else {
                    quote! {}
                }
            }
            ImplNewParameters::UnionType {
                schema,
                union_obj,
                union_ident,
                union_field_set,
            } => generate_from_traits_for_union(
                &schema,
                &union_obj,
                union_ident,
                union_field_set,
            ),
        };

        quote! {
            #impl_entity

            #impl_new

            #impl_json
        }
    }
}

impl From<EnumDecoder> for TokenStream {
    fn from(decoder: EnumDecoder) -> Self {
        let EnumDecoder {
            ident,
            to_enum,
            from_enum,
            values,
            ..
        } = decoder;

        quote! {
            #[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
            pub enum #ident {
                #(#values),*
            }

            impl From<#ident> for String {
                fn from(val: #ident) -> Self {
                    match val {
                        #(#from_enum)*
                        _ => panic!("Unrecognized enum value."),
                    }
                }
            }

            impl From<String> for #ident {
                fn from(val: String) -> Self {
                    match val.as_ref() {
                        #(#to_enum)*
                        _ => panic!("Unrecognized enum value."),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use async_graphql_parser::types::ObjectType;
    use fuel_indexer_lib::graphql::GraphQLSchema;

    #[test]
    fn test_can_create_object_decoder_containing_expected_tokens_from_object_typedef() {
        let schema = r#"
type Person {
    id: ID!
    name: Charfield!
    age: UInt1!
}"#;

        let fields = [("id", "ID"), ("name", "Charfield"), ("age", "UInt1")]
            .iter()
            .map(|(name, typ)| Positioned {
                pos: Pos::default(),
                node: FieldDefinition {
                    description: None,
                    name: Positioned {
                        pos: Pos::default(),
                        node: Name::new(name),
                    },
                    arguments: vec![],
                    ty: Positioned {
                        pos: Pos::default(),
                        node: Type {
                            base: BaseType::Named(Name::new(typ)),
                            nullable: false,
                        },
                    },
                    directives: vec![],
                },
            })
            .collect::<Vec<Positioned<FieldDefinition>>>();
        let typdef = TypeDefinition {
            description: None,
            extend: false,
            name: Positioned {
                pos: Pos::default(),
                node: Name::new("Person"),
            },
            kind: TypeKind::Object(ObjectType {
                implements: vec![],
                fields,
            }),
            directives: vec![],
        };

        let schema = ParsedGraphQLSchema::new(
            "test",
            "test",
            ExecutionSource::Wasm,
            Some(&GraphQLSchema::new(schema.to_string())),
        )
        .unwrap();

        let decoder = ObjectDecoder::from_typedef(&typdef, &schema);
        let tokenstream = TokenStream::from(decoder).to_string();

        // Trying to assert we have every single token expected might be a bit much, so
        // let's just assert that we have the main/primary method and function definitions.
        assert!(tokenstream.contains("pub struct Person"));
        assert!(tokenstream.contains("impl Entity for Person"));
        assert!(tokenstream.contains("impl Person"));
        assert!(
            tokenstream.contains("pub fn new (name : Charfield , age : UInt1 ,) -> Self")
        );
        assert!(tokenstream.contains("pub fn get_or_create (self) -> Self"));
        assert!(tokenstream.contains("fn from_row (mut vec : Vec < FtColumn >) -> Self"));
        assert!(tokenstream.contains("fn to_row (& self) -> Vec < FtColumn >"));
    }
}
