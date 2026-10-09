//! Errors with a stable code for programmatic handling and a message rendered from the
//! catalog (`errors.<code>` in `locales/*.json`) in the current locale.
//!
//! Inside the backend, attach an [`Error`] as anyhow context (`.context(Error::new(..))`);
//! converting the anyhow error back keeps the outermost code and turns the underlying causes
//! into the technical detail appended to the message.

use std::fmt;

use serde::ser::{Serialize, SerializeMap, Serializer};
use ts_rs::TS;

use crate::i18n;

#[derive(Debug, Clone)]
pub struct Error {
    code: &'static str,
    params: Vec<(&'static str, String)>,
    /// Untranslated technical detail (OS / library / server error text).
    detail: Option<String>,
}

impl Error {
    pub fn new(code: &'static str) -> Self {
        Self { code, params: Vec::new(), detail: None }
    }

    pub fn param(mut self, name: &'static str, value: impl fmt::Display) -> Self {
        self.params.push((name, value.to_string()));
        self
    }

    pub fn detail(mut self, detail: impl fmt::Display) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    fn message(&self) -> String {
        let args: Vec<(&str, &dyn fmt::Display)> = self.params.iter().map(|(k, v)| (*k, v as &dyn fmt::Display)).collect();
        let text = i18n::translate(&format!("errors.{}", self.code), &args);
        match &self.detail {
            Some(detail) => format!("{text}: {detail}"),
            None => text,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

/// Serialized for the frontend as `{ code, params, message }`.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let params: std::collections::BTreeMap<_, _> = self.params.iter().map(|(k, v)| (*k, v)).collect();
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("code", self.code)?;
        map.serialize_entry("params", &params)?;
        map.serialize_entry("message", &self.message())?;
        map.end()
    }
}

// What `Error` is serialized as, for the TypeScript bindings only.
/// An error returned by a backend command.
#[derive(TS)]
#[ts(rename = "CommandError")]
#[allow(dead_code)]
struct Serialized {
    #[ts(type = "ErrorCode")]
    code: String,
    params: std::collections::BTreeMap<String, String>,
    /// Already localized by the backend.
    message: String,
}

impl TS for Error {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn docs() -> Option<String> {
        Serialized::docs()
    }

    fn name(cfg: &ts_rs::Config) -> String {
        Serialized::name(cfg)
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        Serialized::inline(cfg)
    }

    fn decl(cfg: &ts_rs::Config) -> String {
        Serialized::decl(cfg)
    }

    fn decl_concrete(cfg: &ts_rs::Config) -> String {
        Serialized::decl_concrete(cfg)
    }

    fn output_path() -> Option<std::path::PathBuf> {
        Serialized::output_path()
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast_ref::<Error>() {
            Some(outer) => {
                let causes: Vec<String> = e.chain().skip(1).map(ToString::to_string).collect();
                let mut error = outer.clone();
                if !causes.is_empty() {
                    error.detail = Some(causes.join(": "));
                }
                error
            }
            None => Error::new("unexpected").detail(format!("{e:#}")),
        }
    }
}

macro_rules! unexpected_from {
    ($($source:ty),*) => {$(
        impl From<$source> for Error {
            fn from(e: $source) -> Self {
                Error::new("unexpected").detail(e)
            }
        }
    )*};
}

unexpected_from!(std::io::Error, serde_json::Error, tauri::Error);

impl From<keyring::Error> for Error {
    fn from(e: keyring::Error) -> Self {
        Error::new("credentialStore").detail(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
