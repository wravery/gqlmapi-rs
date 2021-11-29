use std::{
    sync::{mpsc, Arc, Mutex, PoisonError},
    thread::{self, JoinHandle},
};

mod bindings;
use bindings::{ffi, CompleteContext, NextContext};

use windows::Win32::{
    Foundation::*, System::Threading::GetCurrentThreadId, UI::WindowsAndMessaging::*,
};

enum ServiceCommand {
    Stop,
    ParsedQuery {
        query: String,
        tx_result: mpsc::Sender<Result<i32, String>>,
    },
    DiscardQuery {
        query_id: i32,
    },
    Subscribe {
        query_id: i32,
        operation_name: String,
        variables: String,
        tx_next: mpsc::Sender<String>,
        tx_complete: mpsc::Sender<()>,
        tx_result: mpsc::Sender<Result<i32, String>>,
    },
    Unsubscribe {
        subscription_id: i32,
    },
}

/// Hold the `Bindings` object and automatically clean up when [Service] drops.
struct Service {
    worker: Option<JoinHandle<Result<(), String>>>,
    sender: Mutex<mpsc::Sender<ServiceCommand>>,
    thread_id: u32,
}

impl Service {
    fn new(use_default_profile: bool) -> Arc<Self> {
        let (tx_thread_id, rx_thread_id) = mpsc::channel();
        let (tx_command, rx_command) = mpsc::channel();
        let worker = Some(thread::spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            tx_thread_id
                .send(thread_id)
                .expect("Error sending thread ID");

            let bindings = ffi::make_bindings();
            bindings.startService(use_default_profile);

            loop {
                match Self::wait_with_pump(&rx_command)? {
                    ServiceCommand::Stop => {
                        bindings.stopService();
                        break;
                    }
                    ServiceCommand::ParsedQuery { query, tx_result } => tx_result
                        .send(bindings.parseQuery(&query).map_err(map_exception))
                        .map_err(map_send_error)?,
                    ServiceCommand::DiscardQuery { query_id } => bindings.discardQuery(query_id),
                    ServiceCommand::Subscribe {
                        query_id,
                        operation_name,
                        variables,
                        tx_next,
                        tx_complete,
                        tx_result,
                    } => {
                        let next_context = Box::new(NextContext {
                            callback: Box::new(move |payload| {
                                tx_next.send(payload).expect("Error sending next payload")
                            }),
                            thread_id,
                        });
                        let complete_context = Box::new(CompleteContext {
                            callback: Box::new(move || {
                                let _ = tx_complete.send(());
                            }),
                            thread_id,
                        });
                        let subscription_id = bindings
                            .subscribe(
                                query_id,
                                &operation_name,
                                &variables,
                                next_context,
                                |mut context, payload| {
                                    (context.callback)(payload);
                                    Self::kick_pump(context.thread_id);
                                    context
                                },
                                complete_context,
                                |context| {
                                    (context.callback)();
                                    Self::kick_pump(context.thread_id);
                                },
                            )
                            .map_err(map_exception);
                        tx_result.send(subscription_id).map_err(map_send_error)?
                    }
                    ServiceCommand::Unsubscribe { subscription_id } => {
                        bindings.unsubscribe(subscription_id)
                    }
                }
            }

            Ok(())
        }));
        let thread_id = rx_thread_id.recv().expect("Error receiving thread ID");

        Arc::new(Service {
            worker,
            sender: Mutex::new(tx_command),
            thread_id,
        })
    }

    fn kick_pump(thread_id: u32) {
        unsafe { PostThreadMessageA(thread_id, WM_APP, WPARAM::default(), LPARAM::default()) };
    }

    fn wait_with_pump<T>(rx: &mpsc::Receiver<T>) -> Result<T, String> {
        let mut msg = MSG::default();
        let hwnd = HWND::default();

        loop {
            if let Ok(result) = rx.try_recv() {
                return Ok(result);
            }

            unsafe {
                match GetMessageA(&mut msg, hwnd, 0, 0).0 {
                    -1 => {
                        return Err(format!(
                            "GetMessageA error: {}",
                            windows::core::Error::from_win32().code().0
                        ));
                    }
                    0 => return Err(String::from("Cancelled")),
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageA(&msg);
                    }
                }
            }
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        self.sender
            .lock()
            .map_err(map_lock_error)?
            .send(ServiceCommand::Stop)
            .map_err(map_send_error)?;
        Self::kick_pump(self.thread_id);

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
    /// Shutdown the [GraphQL](https://graphql.org) service and log off from the `MAPI` session.
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
    pub fn parse_query(&self, query: &str) -> Result<Arc<ParsedQuery>, String> {
        let (tx, rx) = mpsc::channel();
        self.0
            .sender
            .lock()
            .map_err(map_lock_error)?
            .send(ServiceCommand::ParsedQuery {
                query: String::from(query),
                tx_result: tx,
            })
            .map_err(map_send_error)?;
        Service::kick_pump(self.0.thread_id);
        let result = rx.recv().map_err(map_recv_error)?;
        Ok(Arc::new(ParsedQuery(self.0.clone(), result?)))
    }

    /// Subscribe to a [GraphQL](https://graphql.org) [ParsedQuery] that was previously parsed with
    /// [parse_query](MAPIGraphQL::parse_query).
    pub fn subscribe(
        &self,
        query: Arc<ParsedQuery>,
        operation_name: &str,
        variables: &str,
    ) -> Mutex<Subscription> {
        Mutex::new(Subscription {
            subscription_id: 0,
            query,
            operation_name: operation_name.into(),
            variables: variables.into(),
        })
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
                .send(ServiceCommand::DiscardQuery { query_id: self.1 })
                .map_err(map_send_error)?;
            Service::kick_pump(self.0.thread_id);
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
pub struct Subscription {
    subscription_id: i32,
    query: Arc<ParsedQuery>,
    operation_name: String,
    variables: String,
}

impl Subscription {
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
            .send(ServiceCommand::Subscribe {
                query_id: self.query.1,
                operation_name: self.operation_name.clone(),
                variables: self.variables.clone(),
                tx_next: next,
                tx_complete: complete,
                tx_result: tx,
            })
            .map_err(map_send_error)?;
        Service::kick_pump(self.query.0.thread_id);
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
                .send(ServiceCommand::Unsubscribe {
                    subscription_id: self.subscription_id,
                })
                .map_err(map_send_error)?;
            Service::kick_pump(self.query.0.thread_id);
            self.subscription_id = 0;
        }
        Ok(())
    }
}

impl Drop for Subscription {
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

        let subscription = gqlmapi.subscribe(query.clone(), "", "");
        let mut locked_subscription = subscription
            .lock()
            .expect("should lock the mut subscription");
        let (tx_next, rx_next) = mpsc::channel();
        let (tx_complete, rx_complete) = mpsc::channel();
        locked_subscription
            .listen(tx_next, tx_complete)
            .expect("subscribes to the query");
        assert_ne!(
            locked_subscription.subscription_id, 0,
            "subscription ID is not 0"
        );
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
