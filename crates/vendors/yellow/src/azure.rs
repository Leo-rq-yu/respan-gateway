//! Azure OpenAI: Chat Completions and Decisions, at a resource's own URL
//! (`https://<resource>.openai.azure.com/openai/v1`).

use respan_gateway_conversation::Output;
use respan_gateway_conversation_http::dialect::{Call, Dialect, Dialected};
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::Filter;
use respan_gateway_decision_http::openai::OpenAiDecisions;

use respan_gateway_vendor_kit::{KeyIn, Vendor, chat};

/// Azure OpenAI. It has no base URL of its own: a resource has one. Its
/// Decisions take the key in `api-key`, as its Chat Completions does.
pub const VENDOR: Vendor = Vendor {
    id: "azure",
    key_env: &["AZURE_OPENAI_API_KEY"],
    formats: &[
        (&Dialected::new(OpenAiChat, Azure), None),
        (&OpenAiDecisions::key_in("api-key"), None),
    ],
    ..Vendor::DEFAULT
};

/// Azure's Chat Completions: its key in `api-key`, and a choice's
/// moderation results only describe it.
#[derive(Debug, Clone, Copy)]
pub struct Azure;

impl Dialect for Azure {
    fn output(&self, call: &Call) -> Option<Box<dyn Filter<Output>>> {
        chat::read(
            call,
            &["content_filter_results", "content_filter_offsets"],
            false,
        )
    }

    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Header("api-key"))
    }
}
