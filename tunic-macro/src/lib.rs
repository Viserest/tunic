use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Data, DataStruct, DeriveInput, Error, Expr, Fields, Ident, LitStr, Path, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
};

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

// Macro argument parsing
struct MacroArgs {
    state: Path,
    table: Expr,
    methods: Vec<Ident>,
    delete_type: Ident,
}

impl Parse for MacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut state: Option<Path> = None;
        let mut table: Option<Expr> = None;
        let mut methods = vec![
            format_ident!("post"),
            format_ident!("get"),
            format_ident!("put"),
            format_ident!("delete"),
        ];

        // Fake: Doesn't delete from table but only sets deleted_at to current time
        // Real: Deletes from table
        let mut delete_type = Ident::new("fake", proc_macro2::Span::call_site());

        // Parse arbitrary order key-value pairs separated by commas or semicolons
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            if key == "state" {
                state = Some(input.parse()?);
            } else if key == "table" {
                table = Some(input.parse()?);
            } else if key == "methods" {
                let content;
                syn::bracketed!(content in input);
                let parsed_methods: Punctuated<Ident, Token![,]> =
                    content.parse_terminated(Ident::parse, Token![,])?;
                methods = parsed_methods.into_iter().collect();
            } else if key == "delete_type" {
                delete_type = input.parse()?;
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    format!("Unknown macro argument: {}", key),
                ));
            }

            // Consume optional trailing comma or semicolon between arguments (if not at the end)
            if input.peek(Token![,]) {
                let _ = input.parse::<Token![,]>();
            } else if input.peek(Token![;]) {
                let _ = input.parse::<Token![;]>();
            }
        }

        Ok(MacroArgs {
            state: state.expect("State argument is required"),
            table: table.expect("Table argument is required"),
            methods,
            delete_type,
        })
    }
}

#[proc_macro_attribute]
pub fn make_crud(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as MacroArgs);
    let mut input = parse_macro_input!(item as DeriveInput);

    let struct_name = &input.ident;
    let vis = &input.vis;

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

    // Filter fields for payloads
    let mut post_fields = Vec::new();
    let mut post_field_names = Vec::new();
    let mut not_post_field_names = Vec::new();
    let mut get_fields = Vec::new();
    let mut put_fields = Vec::new();
    let mut put_field_updates = Vec::new();

    for field in fields.iter_mut() {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        let mut skipped = Vec::new();
        for attr in field.attrs.drain(..) {
            if attr.path().is_ident("skip") {
                let _ = attr.parse_nested_meta(|meta| {
                    if let Some(ident) = meta.path.get_ident() {
                        skipped.push(ident.clone());
                    }
                    Ok(())
                });
            }
        }

        let skip_post = skipped.iter().any(|m| m == "post");
        let skip_get = skipped.iter().any(|m| m == "get");
        let skip_put = skipped.iter().any(|m| m == "put");

        if field_name == "id" && !skip_put {
            put_fields.push(quote! { #field_name: #field_type });
        } else {
            if skip_post {
                not_post_field_names.push(quote! { #field_name });
            } else {
                post_fields.push(quote! { #field_name: #field_type });
                post_field_names.push(quote! { #field_name });
            }
            if !skip_put {
                if let Type::Path(type_path) = field_type {
                    if let Some(segment) = type_path.path.segments.last() {
                        if segment.ident == "Option" {
                            put_fields.push(quote! { #field_name: #field_type });
                            put_field_updates
                                .push(quote! { value.#field_name = payload.#field_name });
                        } else {
                            put_fields.push(quote! { #field_name: Option<#field_type> });
                            put_field_updates
                                .push(quote! { value.#field_name = payload.#field_name.unwrap() });
                        }
                    }
                }
            }
        }

        if !skip_get {
            get_fields.push(quote! { #field_name: #field_type });
        }
    }

    // Generate Handlers based on methods enabled
    let mut struct_tokens = Vec::new();
    let mut handler_tokens = Vec::new();
    let state_type = &args.state;
    let table = &args.table;

    for method in &args.methods {
        if method == "post" {
            let post_payload_name = format_ident!("PostPayload{}", struct_name);
            struct_tokens.push(quote! {
                #[derive(serde::Deserialize)]
                #vis struct #post_payload_name {
                    #(#post_fields),*
                }
            });
            handler_tokens.push(quote! {
                pub async fn handle_post(
                    ::axum::Extension(state): ::axum::Extension<#state_type>,
                    ::axum::Json(payload): ::axum::Json<#post_payload_name>,
                ) -> impl ::axum::response::IntoResponse {
                    let value = #struct_name {
                        id: ::uuid::Uuid::new_v4(),
                        #(#post_field_names: payload.#post_field_names,)*
                        #(#not_post_field_names: ::std::default::Default::default(),)*
                    };
                    if (#table).insert(value.id, value).await.is_some() {
                        tracing::warn!("Entry overwrote a key.");
                    }

                    (
                        ::axum::http::StatusCode::OK,
                        ::axum::Json(::serde_json::json!({"msg": "Created."})),
                    )
                }
            });
        } else if method == "get" {
            let get_payload_name = format_ident!("GetPayload{}", struct_name);
            struct_tokens.push(quote! {
                #[derive(serde::Serialize)]
                #vis struct #get_payload_name {
                    #(#get_fields),*
                }
            });
            handler_tokens.push(quote! {
                pub async fn handle_get(
                    ::axum::Extension(state): ::axum::Extension<#state_type>,
                    ::axum::extract::Query(params): ::axum::extract::Query<::std::collections::HashMap<String, String>>,
                ) -> impl ::axum::response::IntoResponse {
                    // Get optional fields
                    let limit = params
                        .get("limit")
                        .and_then(|f| f.parse::<usize>().ok())
                        .unwrap_or(20)
                        .clamp(1, 100);
                    let offset = params
                        .get("offset")
                        .and_then(|f| f.parse::<usize>().ok())
                        .unwrap_or(0);

                    let mut result = Vec::new();
                    for (_, f) in (#table).iter().await.skip(offset).take(limit) {
                        result.push(::serde_json::json!(*f.lock().await));
                    }

                    (
                        ::axum::http::StatusCode::OK,
                        ::axum::Json(::serde_json::json!({
                            "msg": "Found items.",
                            "length": result.len(),
                            "limit": limit,
                            "offset": offset,
                            "list": result,
                        })),
                    )
                }
            });
        } else if method == "put" {
            let put_payload_name = format_ident!("PutPayload{}", struct_name);
            struct_tokens.push(quote! {
                #[derive(serde::Deserialize)]
                #vis struct #put_payload_name {
                    #(#put_fields),*
                }
            });
            handler_tokens.push(quote! {
                pub async fn handle_put(
                    ::axum::Extension(state): ::axum::Extension<#state_type>,
                    ::axum::Json(payload): ::axum::Json<#put_payload_name>,
                ) -> impl ::axum::response::IntoResponse {
                    let value = match (#table).get_mut(&payload.id).await {
                        Some(a) => a,
                        None => {
                            tracing::warn!("Table put ran and errored: table returned none.");
                            return (
                                ::axum::http::StatusCode::BAD_REQUEST,
                                ::axum::Json(::serde_json::json!({"msg": "Missing 'id' field"})),
                            );
                        }
                    };
                    let mut value = value.lock().await;

                    // If field is optional vs not
                    #(#put_field_updates;)*

                    tracing::info!("Table put ran");
                    (
                        ::axum::http::StatusCode::OK,
                        ::axum::Json(::serde_json::json!({"msg": "Updated whatever"})),
                    )
                }
            });
        } else if method == "delete" {
            let delete_tokens = match args.delete_type.to_string().as_str() {
                "fake" => quote! {
                    let value = match (#table).get_mut(&id).await {
                        Some(a) => a,
                        None => {
                            tracing::warn!("Table delete ran and errored: table returned none.");
                            return (
                                ::axum::http::StatusCode::BAD_REQUEST,
                                ::axum::Json(::serde_json::json!({"msg": "Returned nothing"})),
                            );
                        }
                    };
                    let mut value = value.lock().await;
                    value.deleted_at = #rt_path::unix_now();
                },
                _ => quote! {
                    if (#table).remove(&id).await.is_none() {
                        tracing::warn!("Table delete ran and errored: table returned none.");
                        return (
                            ::axum::http::StatusCode::BAD_REQUEST,
                            ::axum::Json(::serde_json::json!({"msg": "Returned nothing"})),
                        );
                    }
                },
            };
            handler_tokens.push(quote! {
                pub async fn handle_delete(
                    ::axum::Extension(state): ::axum::Extension<#state_type>,
                    ::axum::Json(payload): ::axum::Json<::serde_json::Value>,
                ) -> impl ::axum::response::IntoResponse {
                    let id = match payload
                        .get("id")
                        .and_then(|f| f.as_str())
                        .and_then(|f| f.parse::<::uuid::Uuid>().ok())
                    {
                        Some(a) => a,
                        None => {
                            tracing::warn!("Table delete ran and errored: id not found in payload.");
                            return (
                                ::axum::http::StatusCode::BAD_REQUEST,
                                ::axum::Json(::serde_json::json!({"msg": "Missing 'id' field"})),
                            );
                        }
                    };

                    #delete_tokens

                    tracing::info!("Table delete ran");
                    return (
                        ::axum::http::StatusCode::OK,
                        ::axum::Json(::serde_json::json!({"msg": "Successfully removed"})),
                    );
                }
            });
        }
    }

    let expanded = quote! {
        #input

        #(#struct_tokens)*

        impl #struct_name {
            #(#handler_tokens)*
        }
    };

    TokenStream::from(expanded)
}
