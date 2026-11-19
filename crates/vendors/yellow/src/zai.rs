//! Z.ai: Chat Completions as OpenAI serves it, and images in its dialect.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::Fault;
use respan_gateway_media::image::Input;
use respan_gateway_media_http::openai::{DialectedImages, ImagesDialect};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::Vendor;

/// Where its API is, on its international site.
const BASE_URL: &str = "https://api.z.ai/api/paas/v4";

/// Z.ai.
pub const VENDOR: Vendor = Vendor {
    id: "zai",
    key_env: &["ZAI_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&DialectedImages::new(ZaiImages), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// Z.ai's images: one a call, by URL alone, and no edit. More images, or
/// `b64_json`, or an edit, is refused, 400, before any call; `n: 1` and
/// `response_format: url` are dropped, as it takes neither field.
#[derive(Debug, Clone, Copy)]
pub struct ZaiImages;

impl ImagesDialect for ZaiImages {
    fn fields(&self, fields: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        let refuse = |what: &str| Err(Fault::bad_request(format!("Z.ai {what}")));
        if input.is_edit() {
            return refuse("cannot edit images");
        }
        if input.count.is_some_and(|n| n != 1) {
            return refuse("draws one image a call");
        }
        fields.remove("n");
        match fields.remove("response_format") {
            Some(Value::String(f)) if f != "url" => refuse("answers images only by URL"),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_image_by_url_is_all_it_draws() {
        let mut input = Input::new("p");
        let mut fields = Map::new();
        fields.insert("n".into(), 1.into());
        ZaiImages.fields(&mut fields, &input).expect("fields");
        assert!(fields.is_empty());
        input.count = Some(2);
        assert!(ZaiImages.fields(&mut Map::new(), &input).is_err());
        let mut b64 = Map::new();
        b64.insert("response_format".into(), "b64_json".into());
        assert!(ZaiImages.fields(&mut b64, &Input::new("p")).is_err());
    }
}
