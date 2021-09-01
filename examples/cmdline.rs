extern crate gqlmapi_rs;
use gqlmapi_rs::MAPIGraphQL;

use std::{io::{self, Read}, string::FromUtf8Error, sync::mpsc::{self, RecvError}};

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Utf8(FromUtf8Error),
    GraphQL(String),
    Channel(RecvError),
}

fn main() -> Result<(), Error> {
    println!("Type/paste a query here (finish by pressing Ctrl+Z on an empty line):");
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).map_err(Error::Io)?;
    let query = String::from_utf8(buf).map_err(Error::Utf8)?;
    let results = execute_query(query)?;
    println!("Results: {}", results);
    Ok(())
}

fn execute_query(query: String) -> Result<String, Error> {
    let gqlmapi = MAPIGraphQL::new(true);
    let query = gqlmapi.parse_query(&query).map_err(Error::GraphQL)?;
    let (tx_next, rx_next) = mpsc::channel();
    let (tx_complete, rx_complete) = mpsc::channel();
    let _subscription = gqlmapi.subscribe(
        &query,
        "",
        "",
        Box::new(move |payload| tx_next.send(payload).expect("send next payload")),
        Box::new(move || tx_complete.send(()).expect("send complete")),
    ).map_err(Error::GraphQL)?;
    let results = rx_next.recv().map_err(Error::Channel)?;
    rx_complete.recv().map_err(Error::Channel)?;

    Ok(results)
}
