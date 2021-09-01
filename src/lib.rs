use std::{
    sync::{mpsc, Arc, Mutex, PoisonError},
    thread::{self, JoinHandle},
};

mod bindings;
use bindings::{ffi, CompleteContext, NextContext};

enum ServiceCommand {
    Stop,
    ParsedQuery(String, mpsc::Sender<Result<i32, String>>),
    DiscardQuery(i32),
    Subscribe(
        i32,
        String,
        String,
        mpsc::Sender<String>,
        mpsc::Sender<()>,
        mpsc::Sender<Result<i32, String>>,
    ),
    Unsubscribe(i32),
}

/// Hold the `Bindings` object and automatically clean up when [Service] drops.
struct Service {
    worker: Option<JoinHandle<Result<(), String>>>,
    sender: Mutex<mpsc::Sender<ServiceCommand>>,
}

impl Service {
    fn new(use_default_profile: bool) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Service {
            worker: Some(thread::spawn(move || {
                let bindings = ffi::make_bindings();
                bindings.startService(use_default_profile);

                loop {
                    match rx.recv().map_err(map_recv_error)? {
                        ServiceCommand::Stop => {
                            bindings.stopService();
                            break;
                        }
                        ServiceCommand::ParsedQuery(query, tx_result) => tx_result
                            .send(bindings.parseQuery(&query).map_err(map_exception))
                            .map_err(map_send_error)?,
                        ServiceCommand::DiscardQuery(query_id) => bindings.discardQuery(query_id),
                        ServiceCommand::Subscribe(
                            query_id,
                            operation_name,
                            variables,
                            tx_next,
                            tx_complete,
                            tx_result,
                        ) => {
                            let next_context = Box::new(NextContext(Box::new(move |payload| {
                                tx_next.send(payload).expect("Error sending next payload")
                            })));
                            let complete_context = Box::new(CompleteContext(Box::new(move || {
                                tx_complete.send(()).expect("Error sending complete")
                            })));
                            let subscription_id = bindings
                                .subscribe(
                                    query_id,
                                    &operation_name,
                                    &variables,
                                    next_context,
                                    |mut context, payload| {
                                        context.0(payload);
                                        context
                                    },
                                    complete_context,
                                    |context| context.0(),
                                )
                                .map_err(map_exception);
                            tx_result.send(subscription_id).map_err(map_send_error)?
                        }
                        ServiceCommand::Unsubscribe(subscription_id) => {
                            bindings.unsubscribe(subscription_id)
                        }
                    }
                }

                Ok(())
            })),
            sender: Mutex::new(tx),
        })
    }

    fn stop(&mut self) -> Result<(), String> {
        self.sender
            .lock()
            .map_err(map_lock_error)?
            .send(ServiceCommand::Stop)
            .map_err(map_send_error)?;

        if let Some(worker) = self.worker.take() {
            let result = worker
                .join()
                .map_err(|_| String::from("Error joining the worker"))?;
            result?;
        }

        Ok(())
    }
}

impl Drop for Service {
    /// Shutdown the [GraphQL](https://graphql.org) service log off from the `MAPI` session.
    fn drop(&mut self) {
        self.stop().expect("Unable to stop the service");
    }
}

/// Rust-friendly bindings to [gqlmapi](https://github.com/microsoft/gqlmapi).
pub struct MAPIGraphQL(Arc<Service>);

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
        let (tx, rx) = mpsc::channel();
        self.0
            .sender
            .lock()
            .map_err(map_lock_error)?
            .send(ServiceCommand::ParsedQuery(String::from(query), tx))
            .map_err(map_send_error)?;
        let result = rx.recv().map_err(map_recv_error)?;
        Ok(ParsedQuery(self.0.clone(), result?))
    }

    /// Subscribe to a [GraphQL](https://graphql.org) [ParsedQuery] that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query).
    pub fn subscribe<'a>(
        &self,
        query: &'a ParsedQuery,
        operation_name: &str,
        variables: &str,
    ) -> Subscription<'a> {
        Subscription {
            subscription_id: 0,
            query,
            operation_name: operation_name.into(),
            variables: variables.into(),
        }
    }
}

/// Hold on to a query parsed with [parse_query](MAPIGraphQL::parse_query) and automatically clean
/// up when [ParsedQuery] drops.
pub struct ParsedQuery(Arc<Service>, i32);

impl ParsedQuery {
    fn discard_query(&mut self) -> Result<(), String> {
        if self.1 != 0 {
            self.0
                .sender
                .lock()
                .map_err(map_lock_error)?
                .send(ServiceCommand::DiscardQuery(self.1))
                .map_err(map_send_error)?;
            self.1 = 0;
        }
        Ok(())
    }
}

impl Drop for ParsedQuery {
    /// Cleanup a [GraphQL](https://graphql.org) request document that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query).
    fn drop(&mut self) {
        self.discard_query().expect("Unable to discard query");
    }
}

/// Hold on to an operation subscription created with [subscribe](MAPIGraphQL::subscribe) and
/// automatically clean up when [Subscription] drops..
pub struct Subscription<'a> {
    subscription_id: i32,
    query: &'a ParsedQuery,
    operation_name: String,
    variables: String,
}

impl<'a> Subscription<'a> {
    /// Start listening to the [Subscription] that was previously created with
    /// [subscribe](MAPIGraphQL::subscribe). This will return an [Err(String)](Err) if the
    /// request failed.
    ///
    /// If the specified operation is a `Query` or `Mutation`, it will be evaluated immediately and
    /// result in a pair of calls to `next` and then `complete`.
    ///
    /// If it is a `Subscription` operation, each time the event stream is updated, the payload
    /// will be delivered through another call to `next`. `Subscription` operations will also
    /// invoke `complete` once they are removed by dropping the [Subscription].
    pub fn listen(
        &mut self,
        next: mpsc::Sender<String>,
        complete: mpsc::Sender<()>,
    ) -> Result<(), String> {
        self.unsubscribe()?;

        let (tx, rx) = mpsc::channel();
        self.query
            .0
            .sender
            .lock()
            .map_err(map_lock_error)?
            .send(ServiceCommand::Subscribe(
                self.query.1,
                self.operation_name.clone(),
                self.variables.clone(),
                next,
                complete,
                tx,
            ))
            .map_err(map_send_error)?;
        let result = rx.recv().map_err(map_recv_error)?;

        self.subscription_id = result?;
        Ok(())
    }

    fn unsubscribe(&mut self) -> Result<(), String> {
        if self.subscription_id != 0 {
            self.query
                .0
                .sender
                .lock()
                .map_err(map_lock_error)?
                .send(ServiceCommand::Unsubscribe(self.subscription_id))
                .map_err(map_send_error)?;
            self.subscription_id = 0;
        }
        Ok(())
    }
}

impl<'a> Drop for Subscription<'a> {
    /// Cleanup a `Subscription` that was previously created with [subscribe](MAPIGraphQL::subscribe).
    ///
    /// This is a no-op for `Query` or `Mutation` requests since they deliver 1 immediate result.
    fn drop(&mut self) {
        self.unsubscribe().expect("Unable to unsubscribe");
    }
}

fn map_lock_error<T>(err: PoisonError<T>) -> String {
    format!("Error locking mutex: {}", err)
}

fn map_send_error<T>(err: mpsc::SendError<T>) -> String {
    format!("Error sending message: {}", err)
}

fn map_recv_error(err: mpsc::RecvError) -> String {
    format!("Error receiving message: {}", err)
}

fn map_exception(err: cxx::Exception) -> String {
    String::from(err.what())
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

        let mut subscription = gqlmapi.subscribe(&query, "", "");
        let (tx_next, rx_next) = mpsc::channel();
        let (tx_complete, rx_complete) = mpsc::channel();
        subscription
            .listen(tx_next, tx_complete)
            .expect("subscribes to the query");
        assert_ne!(subscription.subscription_id, 0, "subscription ID is not 0");
        let results = rx_next.recv().expect("should always receive a payload");
        let results = serde_json::from_str::<IntrospectionResults>(&results)
            .expect("payload should fit query");
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
