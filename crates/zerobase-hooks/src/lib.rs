//! Zerobase Hooks — Embedded JavaScript runtime for `pb_hooks/*.pb.js` files.
//!
//! This crate provides an embedded JavaScript runtime (powered by [Boa])
//! that loads and executes hook scripts, mirroring PocketBase's JS hooks
//! system. Scripts register callbacks on record lifecycle events via the
//! `$app` global binding.
//!
//! # Architecture
//!
//! - [`JsHookEngine`] loads `.pb.js` files from a hooks directory, evaluates
//!   them in a sandboxed JS context, and collects registered callbacks.
//! - [`JsHook`] implements the core [`Hook`] trait, bridging JS callbacks
//!   into the Rust hook system.
//! - [`HooksWatcher`] watches the hooks directory for file changes and
//!   triggers hot-reload in development mode.
//! - The `bindings` module exposes the `$app` global with sub-objects like
//!   `$app.dao()`, `$app.logger()`, etc.
//!
//! [Boa]: https://boajs.dev

pub mod bindings;
pub mod engine;
pub mod error;
pub mod live_dao;
pub mod watcher;

pub use bindings::{DaoHandler, DaoRequest, DaoResponse, MailMessage, NoOpDaoHandler};
pub use engine::{JsHook, JsHookEngine, JsRouteRegistration};
pub use error::JsHookError;
pub use live_dao::LiveDaoHandler;
pub use watcher::HooksWatcher;
