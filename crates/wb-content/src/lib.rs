//! Content schema, YAML loading, validation and the pack compiler (doc 04).
//!
//! `content/**/*.yaml` → [`load_dir`] (parse, `extends` + `overrides`,
//! validation) → [`compile`] (SI units, interned strings, typed indices) →
//! [`pack::encode`] (`content.wbpack` with a BLAKE3 hash).

pub mod compile;
pub mod compiled;
pub mod load;
pub mod pack;
#[cfg(feature = "schema")]
pub mod schema;
pub mod source;
mod validate;

pub use compile::compile;
pub use compiled::ContentDb;
pub use load::{ContentError, ContentSet, SourceFile, load_dir, load_files};
pub use pack::{PackHeader, decode, encode};
