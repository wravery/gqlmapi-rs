#[cxx::bridge]
pub mod ffi {
    extern "Rust" {
        type NextContext;
        type CompleteContext;
    }

    unsafe extern "C++" {
        include!("gqlmapi-rs/include/Bindings.h");

        type Bindings;

        fn make_bindings() -> UniquePtr<Bindings>;

        fn startService(&self, useDefaultProfile: bool);
        fn stopService(&self);

        fn parseQuery(&self, query: &str) -> Result<i32>;
        fn discardQuery(&self, queryId: i32);

        #[allow(clippy::too_many_arguments)]
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

pub struct NextContext {
    pub thread_id: u32,
    pub callback: Box<dyn FnMut(String)>,
}

pub struct CompleteContext {
    pub thread_id: u32,
    pub callback: Box<dyn FnOnce()>,
}
