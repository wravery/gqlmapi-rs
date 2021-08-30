#[cxx::bridge]
mod ffi {
    extern "Rust" {
        type NextContext;
        type CompleteContext;
    }

    unsafe extern "C++" {
        include!("gqlmapi_rs/include/Bindings.h");

        type Bindings;

        fn make_bindings() -> UniquePtr<Bindings>;

        fn startService(&self, useDefaultProfile: bool);
        fn stopService(&self);

        fn parseQuery(&self, query: &str) -> Result<i32>;
        fn discardQuery(&self, queryId: i32);

        fn subscribe(
            &self,
            queryId: i32,
            operationName: &str,
            variables: &str,
            nextContext: Box<NextContext>,
            nextCallback: fn(Box<NextContext>, String) -> Box<NextContext>,
            completeContext: Box<CompleteContext>,
            completeCallback: fn(Box<CompleteContext>),
        ) -> Result<i32>;
        fn unsubscribe(&self, subscriptionId: i32);
    }
}

pub struct NextContext(Box<dyn FnMut(String)>);

pub struct CompleteContext(Box<dyn FnOnce()>);

pub struct MAPIGraphQL(cxx::UniquePtr<ffi::Bindings>);

impl MAPIGraphQL {
    pub fn new() -> Self {
        Self(ffi::make_bindings())
    }

    pub fn start_service(&self, use_default_profile: bool) {
        self.bindings().startService(use_default_profile)
    }

    pub fn stop_service(&self) {
        self.bindings().stopService()
    }

    pub fn parse_query(&self, query: &str) -> Result<i32, cxx::Exception> {
        self.bindings().parseQuery(query)
    }

    pub fn discard_query(&self, query_id: i32) {
        self.bindings().discardQuery(query_id)
    }

    pub fn subscribe(
        &self,
        query_id: i32,
        operation_name: &str,
        variables: &str,
        next: Box<dyn FnMut(String)>,
        complete: Box<dyn FnOnce()>,
    ) -> Result<i32, cxx::Exception> {
        self.bindings().subscribe(
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
    }

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
