#![warn(missing_docs)]
//! The media kinds over HTTP:
//!
//! - [`openai`]: OpenAI's media endpoints, as OpenAI and compatible vendors
//!   serve them, one format per kind.
//! - [`gemini`]: Google's Veo, as the Gemini API serves it, a provider of
//!   the video kind.
//! - [`bedrock`]: Amazon Bedrock's InvokeModel, a provider of the embedding
//!   kind.
//! - [`dialect`]: a format as a vendor speaks it, its departures from the
//!   spec in the vendor's [`Dialect`](dialect::Dialect).

pub mod bedrock;
pub mod dialect;
mod edge;
pub mod gemini;
pub mod openai;
