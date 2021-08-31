mod bindings;
use bindings::{ffi, CompleteContext, NextContext};

/// Rust-friendly bindings to [gqlmapi](https://github.com/microsoft/gqlmapi).
pub struct MAPIGraphQL(cxx::UniquePtr<ffi::Bindings>);

impl MAPIGraphQL {
    /// Create a new instance of [MAPIGraphQL].
    pub fn new() -> Self {
        Self(ffi::make_bindings())
    }

    /// Start the [GraphQL](https://graphql.org) service and log on to the `MAPI` session. You can
    /// explicitly call [stop_service](MAPIGraphQL::stop_service) when you're done, or you can let the service
    /// clean itself up when the [MAPIGraphQL] `struct` is dropped.
    pub fn start_service(&self, use_default_profile: bool) {
        self.bindings().startService(use_default_profile)
    }

    /// Shutdown the [GraphQL](https://graphql.org) service log off from the `MAPI` session. This
    /// is a no-op if you have not previously called [start_service](MAPIGraphQL::start_service). It will also
    /// stop automatically when the [MAPIGraphQL] `struct` is dropped.
    pub fn stop_service(&self) {
        self.bindings().stopService()
    }

    /// Parse a [GraphQL](https://graphql.org) request document and return an `i32` `query_id` that
    /// can be used to represent the request in 1 or more calls to [subscribe](MAPIGraphQL::subscribe). You
    /// should explicitly call [discard_query](MAPIGraphQL::discard_query) with the `query_id` when you are
    /// finished, but if you want to implement automatic query caching, save the `query_id` and
    /// reuse it later.
    ///
    /// If the request document cannot be parsed, it will return an [Err(String)](Err).
    ///
    /// All previously parsed documents will be discarded automatically when the [MAPIGraphQL]
    /// `struct` is dropped.
    pub fn parse_query(&self, query: &str) -> Result<i32, String> {
        self.bindings()
            .parseQuery(query)
            .map_err(|exception| exception.what().into())
    }

    /// Cleanup a [GraphQL](https://graphql.org) request document that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query).
    ///
    /// All previously parsed documents will be discarded automatically when the [MAPIGraphQL]
    /// `struct` is dropped.
    pub fn discard_query(&self, query_id: i32) {
        self.bindings().discardQuery(query_id)
    }

    /// Subscribe to a [GraphQL](https://graphql.org) request document that was previously parsed
    /// with [parse_query](MAPIGraphQL::parse_query). This will return an [Err(String)](Err) if the request
    /// failed, including if you have not called [start_service](MAPIGraphQL::start_service) yet.
    ///
    /// If the specified operation is a `Query` or `Mutation`, it will be evaluated immediately and
    /// result in a pair of calls to `next` and then `complete`. `Query` and `Mutation`
    /// operations do not hold on to any state after the immediate results are delivered, so
    /// calling [unsubscribe](MAPIGraphQL::unsubscribe) is a no-op.
    ///
    /// If it is a `Subscription`, each time the event stream is updated, the payload will be
    /// delivered through another call to `next`. `Subscription` operations will also invoke
    /// `complete` once they are removed with a call to [unsubscribe](MAPIGraphQL::unsubscribe). You must
    /// call [unsubscribe](MAPIGraphQL::unsubscribe) to stop receiving `Subscription` events, although any
    /// current `Subscription` requests will be automatically unsubscribed when the [MAPIGraphQL]
    /// `struct` is dropped.
    pub fn subscribe(
        &self,
        query_id: i32,
        operation_name: &str,
        variables: &str,
        next: Box<dyn FnMut(String)>,
        complete: Box<dyn FnOnce()>,
    ) -> Result<i32, String> {
        self.bindings()
            .subscribe(
                query_id,
                operation_name,
                variables,
                Box::new(NextContext(next)),
                |mut context, payload| {
                    context.0(payload);
                    context
                },
                Box::new(CompleteContext(complete)),
                |context| context.0(),
            )
            .map_err(|exception| exception.what().into())
    }

    /// Cleanup a `Subscription` that was previously created with [subscribe](MAPIGraphQL::subscribe).
    ///
    /// This is a no-op for `Query` or `Mutation` requests since they deliver 1 immediate result.
    ///
    /// All current `Subscription` requests will be unsubscribed automatically when the
    /// [MAPIGraphQL] `struct` is dropped.
    pub fn unsubscribe(&self, subscription_id: i32) {
        self.bindings().unsubscribe(subscription_id)
    }

    fn bindings(&self) -> &ffi::Bindings {
        self.0.as_ref().expect("should always be non-null")
    }
}

#[cfg(test)]
mod test {
    extern crate serde;
    use serde::{Deserialize, Serialize};

    use crate::MAPIGraphQL;
    use std::sync::mpsc;

    #[test]
    fn start_stop_service() {
        let gqlmapi = MAPIGraphQL::new();
        gqlmapi.start_service(true);
        gqlmapi.stop_service();
    }

    #[test]
    fn parse_introspection() {
        let gqlmapi = MAPIGraphQL::new();
        let query_id = gqlmapi
            .parse_query(
                r#"query {
            __schema {
                queryType {
                    name
                }
                mutationType {
                    name
                }
                subscriptionType {
                    name
                }
                types {
                    kind
                    name
                }
            }
        }"#,
            )
            .expect("parses the introspection query");
        assert_ne!(0, query_id);
        gqlmapi.discard_query(query_id);
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct OperationType {
        name: String,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct TypeKind {
        kind: String,
        name: String,
    }

    #[allow(non_snake_case)]
    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct Schema {
        queryType: OperationType,
        mutationType: OperationType,
        subscriptionType: OperationType,
        types: Vec<TypeKind>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct Data {
        __schema: Schema,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct IntrospectionResults {
        data: Data,
    }

    #[test]
    fn fetch_introspection() {
        let gqlmapi = MAPIGraphQL::new();
        gqlmapi.start_service(true);

        let query_id = gqlmapi
            .parse_query(
                r#"query {
                    __schema {
                        queryType {
                            name
                        }
                        mutationType {
                            name
                        }
                        subscriptionType {
                            name
                        }
                        types {
                            kind
                            name
                        }
                    }
                }"#,
            )
            .expect("parses the introspection query");
        assert_ne!(0, query_id);

        let (tx_next, rx_next) = mpsc::channel();
        let (tx_complete, rx_complete) = mpsc::channel();
        let subscription_id = gqlmapi
            .subscribe(
                query_id,
                "",
                "",
                Box::new(move |payload| {
                    tx_next
                        .send(
                            serde_json::from_str::<IntrospectionResults>(&payload)
                                .expect("payload should fit query"),
                        )
                        .expect("channel should always send")
                }),
                Box::new(move || tx_complete.send(()).expect("channel should always send")),
            )
            .expect("should always successfully subscribe");
        let results = rx_next.recv().expect("should always receive a payload");
        rx_complete.recv().expect("should always call complete");

        let expected = IntrospectionResults {
            data: Data {
                __schema: Schema {
                    queryType: OperationType {
                        name: "Query".into(),
                    },
                    mutationType: OperationType {
                        name: "Mutation".into(),
                    },
                    subscriptionType: OperationType {
                        name: "Subscription".into(),
                    },
                    types: vec![
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "Boolean".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "Float".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "ID".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "Int".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "String".into(),
                        },
                        TypeKind {
                            kind: "ENUM".into(),
                            name: "__TypeKind".into(),
                        },
                        TypeKind {
                            kind: "ENUM".into(),
                            name: "__DirectiveLocation".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__Schema".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__Type".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__Field".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__InputValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__EnumValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "__Directive".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "DateTime".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "Guid".into(),
                        },
                        TypeKind {
                            kind: "SCALAR".into(),
                            name: "Stream".into(),
                        },
                        TypeKind {
                            kind: "ENUM".into(),
                            name: "SpecialFolder".into(),
                        },
                        TypeKind {
                            kind: "ENUM".into(),
                            name: "PropType".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "ObjectId".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "NamedPropInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "PropValueInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "MultipleItemsInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "PropIdInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "PropertyInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "Order".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "Column".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "CreateItemInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "CreateSubFolderInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "ModifyItemInput".into(),
                        },
                        TypeKind {
                            kind: "INPUT_OBJECT".into(),
                            name: "ModifyFolderInput".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "Attachment".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "NamedPropId".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "PropId".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "PropValue".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "ItemChange".into(),
                        },
                        TypeKind {
                            kind: "UNION".into(),
                            name: "FolderChange".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Query".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Mutation".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Subscription".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Store".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Folder".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Item".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "FileAttachment".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Conversation".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "IntId".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "StringId".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "NamedId".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "IntValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "BoolValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "StringValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "GuidValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "DateTimeValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "BinaryValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "StreamValue".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "Property".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "ItemAdded".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "ItemUpdated".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "ItemRemoved".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "ItemsReloaded".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "FolderAdded".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "FolderUpdated".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "FolderRemoved".into(),
                        },
                        TypeKind {
                            kind: "OBJECT".into(),
                            name: "FoldersReloaded".into(),
                        },
                    ],
                },
            },
        };
        assert_eq!(results, expected);

        gqlmapi.unsubscribe(subscription_id);
        gqlmapi.discard_query(query_id);
        gqlmapi.stop_service();
    }
}
