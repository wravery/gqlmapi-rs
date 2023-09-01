use std::{pin::Pin, str::FromStr};

use serde_json::Value;

#[cxx::bridge]
pub mod ffi {
    extern "Rust" {
        type NextContext;
        type CompleteContext;
    }

    enum ResponseValueType {
        Map,
        List,
        String,
        Null,
        Boolean,
        Int,
        Float,
        EnumValue,
        ID,
        Scalar,
    }

    struct ResponseMapEntry {
        name: UniquePtr<CxxString>,
        value: UniquePtr<ResponseValue>,
    }

    unsafe extern "C++" {
        include!("gqlmapi-rs/include/ResponseTypes.h");
        type ResponseValueType;

        type ResponseValue;

        #[cxx_name = "makeResponseValue"]
        fn make_response_value(value_type: ResponseValueType) -> UniquePtr<ResponseValue>;

        #[cxx_name = "getType"]
        fn get_type(self: &ResponseValue) -> ResponseValueType;
        #[cxx_name = "fromJson"]
        fn from_json(self: Pin<&mut ResponseValue>) -> Pin<&mut ResponseValue>;
        fn reserve(self: Pin<&mut ResponseValue>, additional: usize) -> Result<()>;
        #[cxx_name = "pushMapEntry"]
        fn push_map_entry(
            self: Pin<&mut ResponseValue>,
            name: &str,
            value: UniquePtr<ResponseValue>,
        ) -> Result<bool>;
        #[cxx_name = "pushListEntry"]
        fn push_list_entry(
            self: Pin<&mut ResponseValue>,
            value: UniquePtr<ResponseValue>,
        ) -> Result<()>;
        #[cxx_name = "setString"]
        fn set_string(self: Pin<&mut ResponseValue>, value: &str) -> Result<()>;
        #[cxx_name = "setBool"]
        fn set_bool(self: Pin<&mut ResponseValue>, value: bool) -> Result<()>;
        #[cxx_name = "setInt"]
        fn set_int(self: Pin<&mut ResponseValue>, value: i64) -> Result<()>;
        #[cxx_name = "setFloat"]
        fn set_float(self: Pin<&mut ResponseValue>, value: f64) -> Result<()>;
        #[cxx_name = "releaseMap"]
        fn release_map(
            self: Pin<&mut ResponseValue>,
        ) -> Result<UniquePtr<CxxVector<ResponseMapEntry>>>;
        #[cxx_name = "releaseList"]
        fn release_list(
            self: Pin<&mut ResponseValue>,
        ) -> Result<UniquePtr<CxxVector<ResponseValue>>>;
        #[cxx_name = "releaseString"]
        fn release_string(self: Pin<&mut ResponseValue>) -> Result<UniquePtr<CxxString>>;
        #[cxx_name = "getBool"]
        fn get_bool(self: &ResponseValue) -> Result<bool>;
        #[cxx_name = "getInt"]
        fn get_int(self: &ResponseValue) -> Result<i64>;
        #[cxx_name = "getFloat"]
        fn get_float(self: &ResponseValue) -> Result<f64>;
        #[cxx_name = "releaseScalar"]
        fn release_scalar(self: Pin<&mut ResponseValue>) -> Result<UniquePtr<ResponseValue>>;
    }

    extern "Rust" {
        type JsonValue;

        fn parse_json(json: &str) -> Result<Box<JsonValue>>;
        fn to_json(&mut self) -> Result<String>;

        fn from_value(value: Pin<&mut ResponseValue>) -> Result<Box<JsonValue>>;
        fn into_value(&mut self) -> Result<UniquePtr<ResponseValue>>;
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
    pub callback: Box<dyn FnMut(String)>,
    pub thread_id: u32,
}

pub struct CompleteContext {
    pub callback: Box<dyn FnOnce()>,
    pub thread_id: u32,
}

struct JsonValue(Option<Value>);

fn parse_json(json: &str) -> Result<Box<JsonValue>, String> {
    JsonValue::new(json).map_err(|err| err.to_string())
}

fn from_value(value: Pin<&mut ffi::ResponseValue>) -> Result<Box<JsonValue>, String> {
    JsonValue::try_from(value).map(Box::new)
}

impl JsonValue {
    fn new(json: &str) -> Result<Box<Self>, serde_json::Error> {
        let value = Value::from_str(json)?;
        Ok(Box::new(Self(Some(value))))
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_json(&mut self) -> Result<String, serde_json::Error> {
        let value = self.0.take().unwrap_or(Value::Null);
        serde_json::to_string(&value)
    }

    #[allow(clippy::wrong_self_convention)]
    fn into_value(&mut self) -> Result<cxx::UniquePtr<ffi::ResponseValue>, String> {
        let value = JsonValue(self.0.take());
        value.try_into()
    }
}

impl TryInto<cxx::UniquePtr<ffi::ResponseValue>> for JsonValue {
    type Error = String;

    fn try_into(self) -> Result<cxx::UniquePtr<ffi::ResponseValue>, String> {
        Ok(match self.0 {
            None | Some(Value::Null) => {
                let result = ffi::make_response_value(ffi::ResponseValueType::Null);
                result
                    .as_ref()
                    .ok_or("Failed to allocate Null ResponseValue".to_owned())?;
                result
            }
            Some(Value::Bool(value)) => {
                let mut result = ffi::make_response_value(ffi::ResponseValueType::Boolean);
                result
                    .as_mut()
                    .ok_or("Failed to allocate Bool ResponseValue".to_owned())?
                    .set_bool(value)
                    .map_err(|err| format!("Failed to set Bool: {err}"))?;
                result
            }
            Some(Value::Number(value)) => {
                if value.is_i64() {
                    let mut result = ffi::make_response_value(ffi::ResponseValueType::Int);
                    result
                        .as_mut()
                        .ok_or("Failed to allocate Int ResponseValue".to_owned())?
                        .set_int(value.as_i64().ok_or("Int value out of bounds".to_owned())?)
                        .map_err(|err| format!("Failed to set Int: {err}"))?;
                    result
                } else if value.is_f64() {
                    let mut result = ffi::make_response_value(ffi::ResponseValueType::Float);
                    result
                        .as_mut()
                        .ok_or("Failed to allocate Float ResponseValue".to_owned())?
                        .set_float(
                            value
                                .as_f64()
                                .ok_or("Float value out of bounds".to_owned())?,
                        )
                        .map_err(|err| format!("Failed to set Float: {err}"))?;
                    result
                } else {
                    Err(format!("Unrecognized Number: {value}"))?
                }
            }
            Some(Value::String(value)) => {
                let mut result = ffi::make_response_value(ffi::ResponseValueType::String);
                result
                    .as_mut()
                    .ok_or("Failed to allocate String ResponseValue".to_owned())?
                    .from_json()
                    .set_string(&value)
                    .map_err(|err| format!("Failed to set String: {err}"))?;
                result
            }
            Some(Value::Array(value)) => {
                let mut result = ffi::make_response_value(ffi::ResponseValueType::List);
                let mut pinned = result
                    .as_mut()
                    .ok_or("Failed to allocate List ResponseValue".to_owned())?;

                if !value.is_empty() {
                    pinned.as_mut().reserve(value.len()).map_err(|err| {
                        format!(
                            "Failed to reserve List with capacity {}: {err}",
                            value.len()
                        )
                    })?;
                    for (i, value) in value.into_iter().enumerate() {
                        let value = JsonValue(Some(value)).try_into()?;
                        pinned
                            .as_mut()
                            .push_list_entry(value)
                            .map_err(|err| format!("Failed to push entry {i} into List: {err}"))?;
                    }
                }
                result
            }
            Some(Value::Object(value)) => {
                let mut result = ffi::make_response_value(ffi::ResponseValueType::Map);
                let mut pinned = result
                    .as_mut()
                    .ok_or("Failed to allocate Map ResponseValue".to_owned())?;

                if !value.is_empty() {
                    pinned.as_mut().reserve(value.len()).map_err(|err| {
                        format!("Failed to reserve Map with capacity {}: {err}", value.len())
                    })?;
                    for (name, value) in value.into_iter() {
                        let value = JsonValue(Some(value)).try_into()?;
                        pinned
                            .as_mut()
                            .push_map_entry(&name, value)
                            .map_err(|err| {
                                format!("Failed to push entry \"{name}\" into Map: {err}")
                            })?;
                    }
                }
                result
            }
        })
    }
}

impl TryFrom<Pin<&mut ffi::ResponseValue>> for JsonValue {
    type Error = String;

    fn try_from(mut value: Pin<&mut ffi::ResponseValue>) -> Result<Self, String> {
        Ok(Self(Some(match value.as_mut().get_type() {
            ffi::ResponseValueType::Map => {
                let mut members = value
                    .as_mut()
                    .release_map()
                    .map_err(|err| format!("Failed to release Map entries: {err}"))?;
                let members = members
                    .as_mut()
                    .ok_or("Map ResponseValue returned a null vector".to_owned())?;
                let mut map = serde_json::Map::new();
                for ffi::ResponseMapEntry { name, value } in members.as_mut_slice() {
                    if let (Some(name), Some(value)) = (name.as_ref(), value.as_mut()) {
                        if let (Ok(name), Ok(JsonValue(Some(value)))) =
                            (name.to_str(), value.try_into())
                        {
                            map.insert(name.to_owned(), value);
                        }
                    }
                }
                Value::Object(map)
            }
            ffi::ResponseValueType::List => {
                let mut members = value
                    .as_mut()
                    .release_list()
                    .map_err(|err| format!("Failed to release List entries: {err}"))?;
                let members = members
                    .as_mut()
                    .ok_or("List ResponseValue returned a null vector".to_owned())?;
                let mut list = Vec::new();
                for value in members.iter_mut() {
                    if let Ok(JsonValue(Some(value))) = value.try_into() {
                        list.push(value);
                    }
                }
                Value::Array(list)
            }
            ffi::ResponseValueType::String
            | ffi::ResponseValueType::EnumValue
            | ffi::ResponseValueType::ID => {
                if let Ok(value) = value
                    .as_mut()
                    .release_string()
                    .map_err(|err| format!("Failed to release String: {err}"))?
                    .as_mut()
                    .ok_or("String ResponseValue returned a null value".to_owned())?
                    .to_str()
                {
                    Value::String(value.to_owned())
                } else {
                    Value::Null
                }
            }
            ffi::ResponseValueType::Null => Value::Null,
            ffi::ResponseValueType::Boolean => Value::Bool(
                value
                    .as_mut()
                    .get_bool()
                    .map_err(|err| format!("Failed to get Boolean: {err}"))?,
            ),
            ffi::ResponseValueType::Int => {
                let value = value
                    .as_mut()
                    .get_int()
                    .map_err(|err| format!("Failed to get Int: {err}"))?;
                serde_json::json!(value)
            }
            ffi::ResponseValueType::Float => {
                let value = value
                    .as_mut()
                    .get_float()
                    .map_err(|err| format!("Failed to get Float: {err}"))?;
                serde_json::json!(value)
            }
            ffi::ResponseValueType::Scalar => {
                let mut value = value
                    .as_mut()
                    .release_scalar()
                    .map_err(|err| format!("Failed to release Scalar: {err}"))?;
                let value = value
                    .as_mut()
                    .ok_or("Scalar ResponseValue returned a null value".to_owned())?;
                if let Ok(JsonValue(Some(value))) = value.try_into() {
                    value
                } else {
                    Value::Null
                }
            }
            _ => unreachable!(),
        })))
    }
}
