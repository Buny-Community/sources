//! Drives a built Buny source's production ABI from the host.
//!
//! Sources are not modified in any way: `register_source!` already exports
//! `start`, `get_search_novel_list`, `get_novel_update`,
//! `get_chapter_content_list` and the optional trait entrypoints. We instantiate
//! the wasm with buny-test-runner's host imports and call those exports directly.

use anyhow::{anyhow, Context, Result};
use buny_test_runner::{
    imports,
    libs::{HtmlDocument, StoreItem},
    WasmEnv,
};
use serde::de::DeserializeOwned;
use wasmer::{Function, FunctionEnv, FunctionEnvMut, Instance, Module, Store, Value};

/// `Html::parse_fragment` in scraper produces a tree with no `body` element,
/// while the app's SwiftSoup-backed host does create one. buny-rs's
/// `text_with_newlines` helper parses a fragment and then selects `body`, so
/// against the stock test-runner host it always returns `None` and any source
/// that unwraps it panics. Parsing as a wrapped document restores app
/// behaviour; without this, details and chapter content fail for no real reason.
fn parse_fragment_compat(
    mut env: FunctionEnvMut<WasmEnv>,
    html_ptr: u32,
    html_len: u32,
    base_url_ptr: u32,
    base_url_len: u32,
) -> i32 {
    const INVALID_STRING: i32 = -2;
    let Ok(text) = env.data().read_string(&env, html_ptr, html_len) else {
        return INVALID_STRING;
    };
    let Ok(base_url) = env.data().read_string(&env, base_url_ptr, base_url_len) else {
        return INVALID_STRING;
    };
    let wrapped = if text.to_ascii_lowercase().contains("<body") {
        text
    } else {
        format!("<html><body>{text}</body></html>")
    };
    let html = scraper::Html::parse_document(&wrapped);
    let base_uri = url::Url::parse(&base_url).ok();
    env.data_mut()
        .store
        .store(StoreItem::HtmlDocument(HtmlDocument { html, base_uri }))
}

/// Outcome of one ABI call.
///
/// `register_source!` encodes errors as negative return values, except for
/// `BunyError::Message`, which comes back as a pointer whose first word is -1.
#[derive(Debug)]
pub enum Outcome<T> {
    Ok(T),
    /// -2: the source does not implement this entrypoint.
    Unimplemented,
    /// -3: the source's network request failed.
    RequestError,
    /// Source-supplied error message.
    Message(String),
    /// Any other negative code (-1 is a generic/deserialize failure).
    Code(i32),
}

impl<T> Outcome<T> {
    pub fn into_ok(self, what: &str) -> Result<T> {
        match self {
            Outcome::Ok(v) => Ok(v),
            Outcome::Unimplemented => Err(anyhow!("{what}: unimplemented")),
            Outcome::RequestError => Err(anyhow!("{what}: network request failed")),
            Outcome::Message(m) => Err(anyhow!("{what}: source error: {m}")),
            Outcome::Code(c) => Err(anyhow!("{what}: error code {c}")),
        }
    }
}

pub struct SourceRunner {
    store: Store,
    env: FunctionEnv<WasmEnv>,
    instance: Instance,
    exports: Vec<String>,
}

impl SourceRunner {
    /// Instantiate a source wasm and run its `start` initializer.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let mut store = Store::default();
        let module = Module::from_file(&store, path)
            .with_context(|| format!("loading wasm from {}", path.display()))?;
        let env = FunctionEnv::new(&mut store, WasmEnv::new());
        let mut import_object = imports::generate_imports(&mut store, &env);
        // Only divert networking when a fingerprint-impersonating gateway is
        // configured; otherwise the host's own client is left in place.
        if crate::net::gateway().is_some() {
            import_object.define(
                "net",
                "send",
                Function::new_typed_with_env(&mut store, &env, crate::net::send),
            );
            import_object.define(
                "net",
                "send_all",
                Function::new_typed_with_env(&mut store, &env, crate::net::send_all),
            );
        }
        import_object.define(
            "html",
            "parse_fragment",
            Function::new_typed_with_env(&mut store, &env, parse_fragment_compat),
        );
        let instance = Instance::new(&mut store, &module, &import_object)
            .context("instantiating source (host imports may be out of sync with buny-rs)")?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory);

        let exports = module.exports().map(|e| e.name().to_string()).collect();

        let mut runner = Self { store, env, instance, exports };
        // `start` constructs the source struct; everything else traps without it.
        runner
            .instance
            .exports
            .get_typed_function::<(), ()>(&runner.store, "start")
            .context("source has no `start` export; is this a Buny source?")?
            .call(&mut runner.store)
            .map_err(|e| anyhow!("`start` trapped: {e}{}", runner.drain_stdout_suffix()))?;
        Ok(runner)
    }

    pub fn has_export(&self, name: &str) -> bool {
        self.exports.iter().any(|e| e == name)
    }

    /// Stores a raw string and returns its descriptor.
    ///
    /// Strings are stored unencoded: the wasm side reads them via
    /// `read_string`, which treats the buffer as raw UTF-8, not postcard.
    pub fn descriptor_str(&mut self, value: impl Into<String>) -> i32 {
        self.env
            .as_mut(&mut self.store)
            .store
            .store(StoreItem::String(value.into()))
    }

    /// Stores a postcard-encoded value and returns its descriptor.
    pub fn descriptor<T: serde::Serialize>(&mut self, value: &T) -> Result<i32> {
        self.env
            .as_mut(&mut self.store)
            .store
            .store_encoded(value)
            .map_err(|e| anyhow!("encoding argument: {e}"))
    }

    /// Calls an export and decodes its result.
    pub fn call<T: DeserializeOwned>(&mut self, name: &str, args: &[Value]) -> Result<Outcome<T>> {
        match self.call_raw(name, args)? {
            Outcome::Ok(bytes) => {
                let decoded = postcard::from_bytes::<T>(&bytes).map_err(|e| {
                    anyhow!("`{name}` returned {} bytes that failed to decode: {e}", bytes.len())
                })?;
                Ok(Outcome::Ok(decoded))
            }
            Outcome::Unimplemented => Ok(Outcome::Unimplemented),
            Outcome::RequestError => Ok(Outcome::RequestError),
            Outcome::Message(m) => Ok(Outcome::Message(m)),
            Outcome::Code(c) => Ok(Outcome::Code(c)),
        }
    }

    /// Calls an export and returns the raw postcard payload.
    ///
    /// Used for types buny-rs only implements `Serialize` for (it never needs to
    /// read them back), where decoding would mean mirroring a hand-written impl.
    pub fn call_raw(&mut self, name: &str, args: &[Value]) -> Result<Outcome<Vec<u8>>> {
        let func = self
            .instance
            .exports
            .get_function(name)
            .with_context(|| format!("source does not export `{name}`"))?
            .clone();

        let returned = func
            .call(&mut self.store, args)
            .map_err(|e| anyhow!("`{name}` trapped: {e}{}", self.drain_stdout_suffix()))?;
        let raw = returned
            .first()
            .ok_or_else(|| anyhow!("`{name}` returned no value"))?
            .i32()
            .ok_or_else(|| anyhow!("`{name}` returned a non-i32 value"))?;

        if raw < 0 {
            return Ok(match raw {
                -2 => Outcome::Unimplemented,
                -3 => Outcome::RequestError,
                code => Outcome::Code(code),
            });
        }

        let ptr = raw as u32;
        let env = self.env.as_ref(&self.store);
        let first = env.read_u32(&self.store, ptr)?;

        // A leading -1 marks the BunyError::Message layout:
        // [-1][capacity][total_len][utf-8 message]
        if first == u32::MAX {
            let total = env.read_u32(&self.store, ptr + 8)?;
            let message = env
                .read_string(&self.store, ptr + 12, total.saturating_sub(12))
                .unwrap_or_else(|_| "<unreadable>".into());
            self.free(raw);
            return Ok(Outcome::Message(message));
        }

        // Success layout: [total_len][capacity][postcard payload]
        let bytes = env.read_item_bytes(&self.store, ptr)?;
        self.free(raw);
        Ok(Outcome::Ok(bytes))
    }

    /// Hands a result pointer back to the source so it can reclaim the allocation.
    fn free(&mut self, ptr: i32) {
        if let Ok(f) = self
            .instance
            .exports
            .get_typed_function::<i32, ()>(&self.store, "free_result")
        {
            let _ = f.call(&mut self.store, ptr);
        }
    }

    /// Anything the source logged via `println!`, for attaching to failures.
    pub fn stdout(&self) -> String {
        self.env.as_ref(&self.store).stdout.clone()
    }

    fn drain_stdout_suffix(&self) -> String {
        let out = self.stdout();
        if out.trim().is_empty() {
            String::new()
        } else {
            format!("\n--- source log ---\n{}", out.trim_end())
        }
    }
}
