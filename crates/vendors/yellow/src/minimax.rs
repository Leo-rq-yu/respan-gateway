//! MiniMax: Chat Completions as OpenAI serves it, and images in its
//! dialect.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::Fault;
use respan_gateway_media_http::openai::{DialectedImages, Edits, ImagesDialect};
use serde_json::json;

use respan_gateway_vendor_kit::Vendor;

/// Where its API is, on its international site.
const BASE_URL: &str = "https://api.minimax.io/v1";

/// MiniMax.
pub const VENDOR: Vendor = Vendor {
    id: "minimax",
    key_env: &["MINIMAX_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&DialectedImages::new(MiniMaxImages), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// MiniMax's images (H3): an edit is JSON, its images
/// `[{"image_url"}]` (a data URL for bytes), and takes no mask. A request
/// for several images may answer fewer, each that failed an item with only
/// an `error`, which is kept native.
#[derive(Debug, Clone, Copy)]
pub struct MiniMaxImages;

impl ImagesDialect for MiniMaxImages {
    fn edits(&self) -> Edits {
        Edits::Json {
            path: "/images/edits",
            write: |fields, images, mask| {
                if mask.is_some() {
                    return Err(Fault::bad_request("MiniMax takes no mask"));
                }
                let images = images.into_iter().map(|url| json!({ "image_url": url }));
                fields.insert("images".into(), images.collect());
                Ok(())
            },
        }
    }
}
