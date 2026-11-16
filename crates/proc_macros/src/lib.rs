use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DataStruct, DeriveInput, Error, Fields, Ident, LitStr, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
};

#[proc_macro_derive(TableValueCrud, attributes(tunic))]
pub fn derive_table_value_crud(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let struct_name = input.ident.clone();
    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            _ => panic!("TableValueCrud can only be derived for structs with named fields"),
        },
        _ => panic!("TableValueCrud can only be derived for structs"),
    };

    let mut id_field: Option<(Ident, Type)> = None;
    let mut create_fields = Vec::new();
    let mut update_fields = Vec::new();
    let mut delete_field: Option<(Ident, Type)> = None;

    for field in fields {
        let field_name = field
            .ident
            .clone()
            .expect("TableValueCrud fields must be named");
        let field_type = field.ty.clone();
        let markers = place_markers(&field.attrs);
        for marker in markers {
            match marker.to_string().as_str() {
                "id" => {
                    id_field = Some((field_name.clone(), field_type.clone()));
                }
                "create" => {
                    create_fields.push((field_name.clone(), field_type.clone()));
                }
                "update" => {
                    update_fields.push((field_name.clone(), field_type.clone()));
                }
                "delete" => {
                    delete_field = Some((field_name.clone(), field_type.clone()));
                }
                _ => {}
            }
        }
    }
    let (id_name, id_type) = id_field.expect("missing #[tunic(id)] field");

    let create_payload = format_ident!("CreatePayload");
    let update_payload = format_ident!("UpdatePayload");

    let create_struct_fields = create_fields.iter().map(|(name, ty)| {
        quote! {
            pub #name: #ty
        }
    });

    let create_constructor_args = create_fields.iter().map(|(name, _)| {
        quote! {
            self.#name
        }
    });

    let update_struct_fields = update_fields.iter().map(|(name, ty)| {
        quote! {
            pub #name: #ty
        }
    });

    let update_assignments = update_fields.iter().map(|(name, _)| {
        quote! {
            target.#name = self.#name.clone();
        }
    });

    let delete_impl = if let Some((delete_name, _delete_type)) = delete_field {
        quote! {
            self.#delete_name = crate::unix_now();
            true
        }
    } else {
        quote! {
            false
        }
    };

    let expanded = quote! {
        #[derive(Deserialize)]
        pub struct #create_payload {
            #( #create_struct_fields, )*
        }

        impl ::core::convert::Into<#struct_name> for #create_payload {
            fn into(self) -> #struct_name {
                #struct_name::new(
                    #( #create_constructor_args, )*
                )
            }
        }

        #[derive(Deserialize)]
        pub struct #update_payload {
            #id_name: #id_type,
            #( #update_struct_fields, )*
        }

        impl TableGetKey<#id_type> for #update_payload {
            fn get_key(&self) -> #id_type {
                self.#id_name
            }
        }

        impl TableValueUpdate<#struct_name> for #update_payload {
            fn update(&self, target: &mut #struct_name) {
                #( #update_assignments )*
            }
        }

        impl TableGetKey<#id_type> for #struct_name {
            fn get_key(&self) -> #id_type {
                self.#id_name
            }
        }

        impl TableValueDeleted for #struct_name {
            fn set_deleted(&mut self) -> bool {
                #delete_impl
            }
        }
    };

    TokenStream::from(expanded)
}

fn place_markers(attrs: &[Attribute]) -> Vec<Ident> {
    let mut result = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("tunic") {
            continue;
        }

        let parsed = attr.parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated);

        match parsed {
            Ok(markers) => {
                result.extend(markers);
            }
            Err(error) => {
                panic!("invalid #[tunic(...)] attribute: {error}");
            }
        }
    }

    result
}

// Helper struct to parse `#[make_user(namespace = "UUID")]`
struct MakeUserArgs {
    namespace_lit: LitStr,
}

impl Parse for MakeUserArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        if ident != "namespace" {
            return Err(Error::new(ident.span(), "expected `namespace`"));
        }
        let _: Token![=] = input.parse()?;
        let namespace_lit: LitStr = input.parse()?;
        Ok(MakeUserArgs { namespace_lit })
    }
}

/// Make a struct become a valid user
///
/// Attributes:
/// #[username], and #[password] must be provided. This allows custom field names to be used.
#[proc_macro_attribute]
pub fn make_user(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse macro attributes: #[make_user(namespace = "...")]
    let args = parse_macro_input!(attr as MakeUserArgs);
    let namespace_str = args.namespace_lit.value();

    // Parse the input struct
    let mut input = parse_macro_input!(item as DeriveInput);
    let struct_name = &input.ident;

    // Ensure it is a struct with named fields
    let fields = match &mut input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => &mut fields.named,
        _ => {
            return Error::new_spanned(
                struct_name,
                "#[make_user] can only be applied to structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    // Track username and password fields
    let mut id_field_name = None;
    let mut username_field_name = None;
    let mut password_field_name = None;

    for field in fields.iter_mut() {
        // let mut is_id = false;
        // let mut is_username = false;
        // let mut is_password = false;

        for attr in field.attrs.drain(..) {
            if attr.path().is_ident("id") {
                if id_field_name.is_some() {
                    return Error::new_spanned(&field.ident, "Only one #[id] attribute is allowed")
                        .to_compile_error()
                        .into();
                }
                // is_id = true;
                id_field_name = field.ident.clone();
            } else if attr.path().is_ident("username") {
                if username_field_name.is_some() {
                    return Error::new_spanned(
                        &field.ident,
                        "Only one #[username] attribute is allowed",
                    )
                    .to_compile_error()
                    .into();
                }
                // is_username = true;
                username_field_name = field.ident.clone();
            } else if attr.path().is_ident("password") {
                if password_field_name.is_some() {
                    return Error::new_spanned(
                        &field.ident,
                        "Only one #[password] attribute is allowed",
                    )
                    .to_compile_error()
                    .into();
                }
                // is_password = true;
                password_field_name = field.ident.clone();
            }
        }

        // Optional: Ensure id, username, or password are of type String or compatible
        // if is_id {}
        // if is_username || is_password {}
    }

    let id_ident = match id_field_name {
        Some(name) => name,
        None => {
            return Error::new_spanned(struct_name, "Struct must include a #[id] field")
                .to_compile_error()
                .into();
        }
    };

    let username_ident = match username_field_name {
        Some(name) => name,
        None => {
            return Error::new_spanned(struct_name, "Struct must include a #[username] field")
                .to_compile_error()
                .into();
        }
    };

    let password_ident = match password_field_name {
        Some(name) => name,
        None => {
            return Error::new_spanned(struct_name, "Struct must include a #[password] field")
                .to_compile_error()
                .into();
        }
    };

    // Collect all other fields (excluding id, username, and password) for Default initialization
    let mut default_field_assignments = Vec::new();
    for field in fields.iter() {
        let ident = field.ident.as_ref().unwrap();
        if ident != &id_ident && ident != &username_ident && ident != &password_ident {
            default_field_assignments.push(quote! {
                #ident: Default::default()
            });
        }
    }

    let lib_name = crate_name("tunic").expect("tunic is present in your Cargo.toml");
    let rt_path = match lib_name {
        FoundCrate::Itself => {
            quote! { crate }
        }
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote! { #ident }
        }
    };

    // 3. Make an impl block with a `new` function
    let expanded = quote! {
        #input

        impl #struct_name {
            pub fn new(
                #username_ident: impl Into<String>,
                #password_ident: impl Into<String>,
            ) -> Self {
                // Generate a UUID using a namespace and the username (or arbitrary bytes)
                let namespace_uuid = ::uuid::Uuid::parse_str(#namespace_str)
                    .expect("Invalid UUID string provided in #[make_user(namespace)]");

                let username_string = #username_ident.into();
                let generated_id = ::uuid::Uuid::new_v5(&namespace_uuid, username_string.as_bytes());

                Self {
                    #id_ident: generated_id,
                    #username_ident: username_string,
                    #password_ident: #rt_path::user::password_hash(#password_ident.into()),
                    #(#default_field_assignments),*
                }
            }
        }

        impl #rt_path::user::UserTrait for #struct_name {
            fn get_id(&self) -> &Uuid {
                &self.#id_ident
            }
            fn get_username(&self) -> &String {
                &self.#username_ident
            }
            fn set_username(&mut self, #username_ident: impl Into<String>) {
                self.#username_ident = #username_ident.into();
            }
            fn get_password(&self) -> &String {
                &self.#password_ident
            }
            fn set_password(&mut self, #password_ident: impl Into<String>) {
                self.#password_ident = #rt_path::user::password_hash(#password_ident);
            }
        }
    };

    TokenStream::from(expanded)
}
