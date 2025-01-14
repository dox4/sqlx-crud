use inflector::Inflector;
use proc_macro::{self, TokenStream};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{
    parse_macro_input, Attribute, Data, DataStruct, DeriveInput, Expr, ExprLit, Field, Fields,
    FieldsNamed, Ident, LitStr, Token,
};

#[proc_macro_derive(
    SqlxCrud,
    attributes(database, external_id, id, ignore_when, auto_increment, deleted_with,)
)]
pub fn derive(input: TokenStream) -> TokenStream {
    let DeriveInput {
        ident, data, attrs, ..
    } = parse_macro_input!(input);
    match data {
        Data::Struct(DataStruct {
            fields: Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => {
            let config = Config::new(&attrs, &ident, &named);
            let static_model_schema = build_static_model_schema(&config);
            let sqlx_crud_impl = build_sqlx_crud_impl(&config);

            quote! {
                #static_model_schema
                #sqlx_crud_impl
            }
            .into()
        }
        _ => panic!("this derive macro only works on structs with named fields"),
    }
}

fn build_static_model_schema(config: &Config) -> TokenStream2 {
    let crate_name = &config.crate_name;
    let model_schema_ident = &config.model_schema_ident;
    let table_name = &config.table_name;

    let id_column = config.id_column_ident.to_string();
    let columns_len = config.named.iter().count();
    let columns = config
        .named
        .iter()
        .flat_map(|f| &f.ident)
        .map(|f| LitStr::new(format!("{}", f).as_str(), f.span()));

    let sql_queries = build_sql_queries(config);

    quote! {
        #[automatically_derived]
        static #model_schema_ident: #crate_name::schema::Metadata<'static, #columns_len> = #crate_name::schema::Metadata {
            table_name: #table_name,
            id_column: #id_column,
            columns: [#(#columns),*],
            #sql_queries
        };
    }
}

fn build_sql_queries(config: &Config) -> TokenStream2 {
    let table_name = config.quote_ident(&config.table_name);
    let id_column = format!(
        "{}.{}",
        &table_name,
        config.quote_ident(&config.id_column_ident.to_string())
    );

    // build select sql
    let (select_sql, select_by_id_sql) = build_select_sql(config, &table_name, &id_column);
    // build insert sql
    let insert_sql = build_insert_sql(config, &table_name);
    // build update sql
    let update_by_id_sql = build_update_sql(config, &table_name, &id_column);
    // build delete sql
    let delete_by_id_sql = build_delete_sql(config, &table_name, &id_column);
    quote! {
        select_sql: #select_sql,
        select_by_id_sql: #select_by_id_sql,
        insert_sql: #insert_sql,
        update_by_id_sql: #update_by_id_sql,
        delete_by_id_sql: #delete_by_id_sql,
    }
}

fn build_select_sql(config: &Config, table_name: &String, id_column: &String) -> (String, String) {
    let column_list = config
        .named
        .iter()
        .flat_map(|f| &f.ident)
        .map(|i| format!("{}.{}", &table_name, config.quote_ident(&i.to_string())))
        .collect::<Vec<_>>()
        .join(", ");
    match config.delete_ident() {
        Some(ident) => {
            let select_sql = format!(
                "SELECT {} FROM {} WHERE {} IS NULL",
                column_list,
                table_name,
                config.quote_ident(ident.as_str())
            );
            let select_by_id_sql = format!(
                "SELECT {} FROM {} WHERE {} = ? AND {} IS NULL LIMIT 1",
                column_list,
                table_name,
                id_column,
                config.quote_ident(ident.as_str())
            );
            (select_sql, select_by_id_sql)
        }
        None => {
            let select_sql = format!("SELECT {} FROM {}", column_list, table_name);
            let select_by_id_sql = format!(
                "SELECT {} FROM {} WHERE {} = ? LIMIT 1",
                column_list, table_name, id_column
            );
            (select_sql, select_by_id_sql)
        }
    }
}

fn build_insert_sql(config: &Config, table_name: &String) -> String {
    let insert_bind_cnt = config.insert_fields.len();
    let insert_sql_binds = (0..insert_bind_cnt)
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let insert_column_list = config
        .insert_fields
        .iter()
        .flat_map(|f| &f.ident)
        // .filter(|i| config.external_id || *i != &config.id_column_ident)
        .map(|i| config.quote_ident(&i.to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table_name, insert_column_list, insert_sql_binds
    )
}

fn build_update_sql(config: &Config, table_name: &String, id_column: &String) -> String {
    let update_sql_binds = config
        .update_fields
        .iter()
        .flat_map(|f| &f.ident)
        .filter(|i| *i != &config.id_column_ident)
        .map(|i| format!("{} = ?", config.quote_ident(&i.to_string())))
        .collect::<Vec<_>>()
        .join(", ");

    match config.delete_ident() {
        Some(field) => format!(
            "UPDATE {} SET {} WHERE {} = ? AND {} IS NULL",
            table_name,
            update_sql_binds,
            id_column,
            config.quote_ident(field.as_str())
        ),
        None => format!(
            "UPDATE {} SET {} WHERE {} = ?",
            table_name, update_sql_binds, id_column
        ),
    }
}
fn build_delete_sql(config: &Config, table_name: &String, id_column: &String) -> String {
    config.delete_field.map_or_else(
        || format!("DELETE FROM {} WHERE {} = ?", table_name, id_column),
        |field| {
            let ident = field
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("deleted_with"))
                .unwrap()
                .meta
                .require_name_value()
                .expect("deleted_with must has a value like `#[deleted_with = \"now()\"]`")
                .clone()
                .value;
            let deleted = match ident {
                Expr::Lit(ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) => lit_str.value(),
                _ => panic!("deleted_with must be a string"),
            };
            let quoted_deleted_field =
                config.quote_ident(&field.ident.as_ref().unwrap().to_string());
            format!(
                "UPDATE {} SET {} = {} WHERE {} = ? AND {} IS NULL",
                table_name, quoted_deleted_field, deleted, id_column, quoted_deleted_field
            )
        },
    )
}

fn build_sqlx_crud_impl(config: &Config) -> TokenStream2 {
    let crate_name = &config.crate_name;
    let ident = &config.ident;
    let model_schema_ident = &config.model_schema_ident;
    let db_ty = config.db_ty.sqlx_db();
    let id_column_ident = &config.id_column_ident;

    let id_ty = config
        .named
        .iter()
        .find(|f| f.ident.as_ref() == Some(id_column_ident))
        .map(|f| &f.ty)
        .expect("the id type");

    let insert_query_args = config
        .insert_fields
        .iter()
        .flat_map(|f| &f.ident)
        // .filter(|i| config.external_id || *i != &config.id_column_ident)
        .map(|i| quote! { args.add(self.#i); });

    let insert_query_size = config
        .insert_fields
        .iter()
        .flat_map(|f| &f.ident)
        // .filter(|i| config.external_id || *i != &config.id_column_ident)
        .map(|i| quote! { ::sqlx::encode::Encode::<#db_ty>::size_hint(&self.#i) });

    let update_query_args = config
        .update_fields
        .iter()
        .flat_map(|f| &f.ident)
        // .filter(|i| *i != &config.id_column_ident)
        .map(|i| quote! { args.add(self.#i); });

    let update_query_args_id = quote! { args.add(self.#id_column_ident); };

    let update_query_size = config
        .update_fields
        .iter()
        .flat_map(|f| &f.ident)
        .map(|i| quote! { ::sqlx::encode::Encode::<#db_ty>::size_hint(&self.#i) });

    quote! {
        #[automatically_derived]
        impl #crate_name::traits::Schema for #ident {
            type Id = #id_ty;

            fn table_name() -> &'static str {
                #model_schema_ident.table_name
            }

            fn id(&self) -> Self::Id {
                self.#id_column_ident
            }

            fn id_column() -> &'static str {
                #model_schema_ident.id_column
            }

            fn columns() -> &'static [&'static str] {
                &#model_schema_ident.columns
            }

            fn select_sql() -> &'static str {
                #model_schema_ident.select_sql
            }

            fn select_by_id_sql() -> &'static str {
                #model_schema_ident.select_by_id_sql
            }

            fn insert_sql() -> &'static str {
                #model_schema_ident.insert_sql
            }

            fn update_by_id_sql() -> &'static str {
                #model_schema_ident.update_by_id_sql
            }

            fn delete_by_id_sql() -> &'static str {
                #model_schema_ident.delete_by_id_sql
            }
        }

        #[automatically_derived]
        impl<'e> #crate_name::traits::Crud<'e, &'e ::sqlx::pool::Pool<#db_ty>> for #ident {
            fn insert_args(self) -> <#db_ty as ::sqlx::database::HasArguments<'e>>::Arguments {
                use ::sqlx::Arguments as _;
                let mut args = <#db_ty as ::sqlx::database::HasArguments<'e>>::Arguments::default();
                args.reserve(1usize, #(#insert_query_size)+*);
                #(#insert_query_args)*
                args
            }

            fn update_args(self) -> <#db_ty as ::sqlx::database::HasArguments<'e>>::Arguments {
                use ::sqlx::Arguments as _;
                let mut args = <#db_ty as ::sqlx::database::HasArguments<'e>>::Arguments::default();
                args.reserve(1usize, #(#update_query_size)+*);
                #(#update_query_args)*
                #update_query_args_id
                args
            }
        }
    }
}

#[allow(dead_code)] // Usage in quote macros aren't flagged as used
struct Config<'a> {
    ident: &'a Ident,
    named: &'a Punctuated<Field, Comma>,
    crate_name: TokenStream2,
    db_ty: DbType,
    model_schema_ident: Ident,
    table_name: String,
    id_column_ident: Ident,
    external_id: bool,
    // additional fields
    update_fields: Vec<&'a Field>,
    insert_fields: Vec<&'a Field>,
    delete_field: Option<&'a Field>,
}

impl<'a> Config<'a> {
    fn new(attrs: &[Attribute], ident: &'a Ident, named: &'a Punctuated<Field, Comma>) -> Self {
        let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();
        let is_doctest = std::env::vars()
            .any(|(k, _)| k == "UNSTABLE_RUSTDOC_TEST_LINE" || k == "UNSTABLE_RUSTDOC_TEST_PATH");
        let crate_name = if !is_doctest && crate_name == "sqlx-crud" {
            quote! { crate }
        } else {
            quote! { ::sqlx_crud }
        };

        let delete_field = named.iter().find(|f| {
            f.attrs
                .iter()
                .find(|attr| attr.path().is_ident("deleted_with"))
                .is_some()
        });
        let db_ty = DbType::new(attrs);

        let model_schema_ident =
            format_ident!("{}_SCHEMA", ident.to_string().to_screaming_snake_case());

        let table_name = ident.to_string().to_table_case();

        // Search for a field with the #[id] attribute
        let id_field = named
            .iter()
            .find(|f| f.attrs.iter().any(|a| a.path().is_ident("id")))
            .unwrap_or_else(|| named.iter().next().expect("the first field."));
        let id_auto_increment = id_field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("auto_increment"));
        // .and_then(|f| f.ident.as_ref());
        // Otherwise default to the first field as the "id" column
        let id_column_ident = id_field.clone().ident.unwrap().clone();
        let external_id = attrs.iter().any(|a| a.path().is_ident("external_id"));

        let insert_fields = named
            .iter()
            .filter(|f| {
                let is_not_id =
                    f.ident.as_ref().unwrap().to_string() != id_column_ident.to_string();
                let no_ignore_attr = !f.attrs.iter().any(|attr| Self::has_ignore(attr, "insert"));
                if id_auto_increment {
                    is_not_id && no_ignore_attr
                } else {
                    no_ignore_attr
                }
            })
            .collect();
        let update_fields = named
            .iter()
            .filter(|f| {
                f.ident.as_ref().unwrap().to_string() != id_column_ident.to_string()
                    && !f.attrs.iter().any(|attr| Self::has_ignore(attr, "update"))
            })
            .collect();

        Self {
            ident,
            named,
            crate_name,
            db_ty,
            model_schema_ident,
            table_name,
            id_column_ident,
            external_id,
            insert_fields,
            update_fields,
            delete_field,
        }
    }

    fn quote_ident(&self, ident: &str) -> String {
        self.db_ty.quote_ident(ident)
    }

    fn delete_ident(&self) -> Option<String> {
        self.delete_field
            .map(|f| f.ident.clone().unwrap().to_string())
    }

    fn has_ignore(attr: &Attribute, target: &str) -> bool {
        attr.path().is_ident("ignore_when")
            && attr
                .meta
                .require_list()
                .expect("ignore_when must be a list, like #[ignore_when(...)]")
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .unwrap()
                .iter()
                .any(|m| m.is_ident(target))
    }
}

enum DbType {
    Any,
    Mssql,
    MySql,
    Postgres,
    Sqlite,
}

impl From<&str> for DbType {
    fn from(db_type: &str) -> Self {
        match db_type {
            "Any" | "any" => Self::Any,
            "Mssql" | "mssql" => Self::Mssql,
            "MySql" | "mysql" => Self::MySql,
            "Postgres" | "postgres" => Self::Postgres,
            "Sqlite" | "sqlite" => Self::Sqlite,
            _ => panic!("unknown #[database] type {}", db_type),
        }
    }
}
impl Into<DbType> for String {
    fn into(self) -> DbType {
        DbType::from(self.as_str())
    }
}

impl DbType {
    fn new(attrs: &[Attribute]) -> Self {
        attrs
            .iter()
            .find(|a| a.path().is_ident("database"))
            .map(|a| {
                a.parse_args::<syn::Path>()
                    .expect("database should be a name like #[database(sqlite)].")
                    .get_ident()
                    .unwrap()
                    .to_string()
            })
            .unwrap_or_else(|| {
                DEFAULT_DB_TYPE
                    .map(|db_type| db_type.into())
                    .unwrap_or("sqlite".to_string())
            })
            .into()
    }

    fn sqlx_db(&self) -> TokenStream2 {
        match self {
            Self::Any => quote! { ::sqlx::Any },
            Self::Mssql => quote! { ::sqlx::Mssql },
            Self::MySql => quote! { ::sqlx::MySql },
            Self::Postgres => quote! { ::sqlx::Postgres },
            Self::Sqlite => quote! { ::sqlx::Sqlite },
        }
    }

    fn quote_ident(&self, ident: &str) -> String {
        match self {
            Self::Any => format!(r#""{}""#, &ident),
            Self::Mssql => format!(r#""{}""#, &ident),
            Self::MySql => format!("`{}`", &ident),
            Self::Postgres => format!(r#""{}""#, &ident),
            Self::Sqlite => format!(r#""{}""#, &ident),
        }
    }
}

#[cfg(feature = "default_mysql")]
static DEFAULT_DB_TYPE: Option<&str> = Some("mysql");
#[cfg(not(any(feature = "default_mysql")))]
static DEFAULT_DB_TYPE: Option<&str> = None;
