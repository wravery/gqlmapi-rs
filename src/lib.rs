use std::rc::Rc;

mod bindings;
use bindings::{ffi, CompleteContext, NextContext};

/// Hold the [Bindings](ffi::Bindings) object and automatically clean up when [Service] drops.
struct Service(cxx::UniquePtr<ffi::Bindings>);

impl Service {
    fn new(use_default_profile: bool) -> Rc<Self> {
        let instance = Rc::new(Self(ffi::make_bindings()));
        instance.bindings().startService(use_default_profile);
        instance
    }

    fn bindings(&self) -> &ffi::Bindings {
        self.0.as_ref().expect("should always be non-null")
    }
}

impl Drop for Service {
    /// Shutdown the [GraphQL](https://graphql.org) service log off from the `MAPI` session.
    fn drop(&mut self) {
        self.0.stopService();
    }
}

/// Rust-friendly bindings to [gqlmapi](https://github.com/microsoft/gqlmapi).
pub struct MAPIGraphQL(Rc<Service>);

impl MAPIGraphQL {
    /// Start the [GraphQL](https://graphql.org) service and log on to the `MAPI` session.
    pub fn new(use_default_profile: bool) -> Self {
        Self(Service::new(use_default_profile))
    }

    /// Parse a [GraphQL](https://graphql.org) request document and return a [ParsedQuery] that can
    /// be used to represent the request in 1 or more calls to [subscribe](MAPIGraphQL::subscribe).
    ///
    /// If the request document cannot be parsed, it will return an [Err(String)](Err).
    pub fn parse_query(&self, query: &str) -> Result<ParsedQuery, String> {
        match self.0.bindings().parseQuery(query) {
            Ok(query_id) => Ok(ParsedQuery(self.0.clone(), query_id)),
            Err(exception) => Err(exception.what().into()),
        }
    }

    /// Subscribe to a [GraphQL](https://graphql.org) [ParsedQuery] that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query). This will return an [Err(String)](Err) if the
    /// request failed.
    ///
    /// If the specified operation is a `Query` or `Mutation`, it will be evaluated immediately and
    /// result in a pair of calls to `next` and then `complete`.
    ///
    /// If it is a `Subscription` operation, each time the event stream is updated, the payload
    /// will be delivered through another call to `next`. `Subscription` operations will also
    /// invoke `complete` once they are removed by dropping the [Subscription].
    pub fn subscribe(
        &self,
        query: &ParsedQuery,
        operation_name: &str,
        variables: &str,
        next: Box<dyn FnMut(String)>,
        complete: Box<dyn FnOnce()>,
    ) -> Result<Subscription, String> {
        match self.0.bindings().subscribe(
            query.1,
            operation_name,
            variables,
            Box::new(NextContext(next)),
            |mut context, payload| {
                context.0(payload);
                context
            },
            Box::new(CompleteContext(complete)),
            |context| context.0(),
        ) {
            Ok(subscription_id) => Ok(Subscription(self.0.clone(), subscription_id)),
            Err(exception) => Err(exception.what().into()),
        }
    }
}

/// Hold on to a query parsed with [parse_query](MAPIGraphQL::parse_query) and automatically clean
/// up when [ParsedQuery] drops.
pub struct ParsedQuery(Rc<Service>, i32);

impl Drop for ParsedQuery {
    /// Cleanup a [GraphQL](https://graphql.org) request document that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query).
    fn drop(&mut self) {
        self.0.bindings().discardQuery(self.1)
    }
}

/// Hold on to an operation subscription created with [subscribe](MAPIGraphQL::subscribe) and
/// automatically clean up when [Subscription] drops..
pub struct Subscription(Rc<Service>, i32);

impl Drop for Subscription {
    /// Cleanup a `Subscription` that was previously created with [subscribe](MAPIGraphQL::subscribe).
    ///
    /// This is a no-op for `Query` or `Mutation` requests since they deliver 1 immediate result.
    fn drop(&mut self) {
        self.0.bindings().unsubscribe(self.1)
    }
}

#[cfg(test)]
mod test {
    extern crate serde;
    use serde::{Deserialize, Serialize};

    use crate::MAPIGraphQL;
    use std::sync::mpsc;

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
        let gqlmapi = MAPIGraphQL::new(true);
        let query = gqlmapi
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
        assert_ne!(0, query.1, "query ID is not 0");

        let (tx_next, rx_next) = mpsc::channel();
        let (tx_complete, rx_complete) = mpsc::channel();
        let subscription = gqlmapi
            .subscribe(
                &query,
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
            .expect("subscribes to the query");
        assert_ne!(subscription.1, 0, "subscription ID is not 0");
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
        assert_eq!(results, expected, "results should match expected snapshot");
    }
}
