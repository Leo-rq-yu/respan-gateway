//! OpenRouter: Chat Completions, images and decisions, for many vendors'
//! models.

use respan_gateway_conversation::Output;
use respan_gateway_conversation_http::dialect::{Call, Dialect, Dialected};
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::{Fault, Filter};
use respan_gateway_decision_http::SystemOne;
use respan_gateway_media::image::Input;
use respan_gateway_media_http::openai::{
    DialectedImages, Edits, ImagesDialect, aspect_ratio, resolution,
};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::{Vendor, chat};

/// Where its API is.
const BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Where its System One is, in alpha: `/api/alpha/systemone`.
const ALPHA: &str = "https://openrouter.ai/api/alpha";

/// OpenRouter. Its models' names are `<vendor>/<model>`: a configured
/// provider named `<vendor>` takes the name first.
pub const VENDOR: Vendor = Vendor {
    id: "openrouter",
    key_env: &["OPENROUTER_API_KEY"],
    formats: &[
        (&Dialected::new(OpenAiChat, OpenRouter), Some(BASE_URL)),
        (&DialectedImages::new(OpenRouterImages), Some(BASE_URL)),
        (&SystemOne, Some(ALPHA)),
    ],
    ..Vendor::DEFAULT
};

/// OpenRouter's Chat Completions: a choice's `native_finish_reason` only
/// describes it.
#[derive(Debug, Clone, Copy)]
pub struct OpenRouter;

impl Dialect for OpenRouter {
    fn output(&self, call: &Call) -> Option<Box<dyn Filter<Output>>> {
        chat::read(call, &["native_finish_reason"], false)
    }
}

/// The aspect ratios OpenRouter names.
const RATIOS: &[&str] = &[
    "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9", "9:21", "1:2", "2:1",
];

/// OpenRouter's resolutions, by their longer side.
const TIERS: &[(&str, u32)] = &[
    ("512", 512),
    ("768", 768),
    ("1K", 1024),
    ("1.5K", 1536),
    ("2K", 2048),
    ("4K", 4096),
];

/// OpenRouter's images:
///
/// - One endpoint, `/images`, draws and edits.
/// - A size is its nearest `aspect_ratio` and `resolution`, which every
///   model behind it takes, rather than pixels, which only some do.
/// - An edit's images are `input_references`, URLs or data URLs; it takes
///   no mask.
#[derive(Debug, Clone, Copy)]
pub struct OpenRouterImages;

impl ImagesDialect for OpenRouterImages {
    fn generations(&self) -> &'static str {
        "/images"
    }

    fn edits(&self) -> Edits {
        Edits::Json {
            path: "/images",
            write: |fields, images, mask| {
                if mask.is_some() {
                    return Err(Fault::bad_request("OpenRouter takes no mask"));
                }
                fields.insert("input_references".into(), images.into());
                Ok(())
            },
        }
    }

    fn fields(&self, fields: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        fields.remove("size");
        if let Some((ratio, long)) = aspect_ratio(input.size.as_deref(), RATIOS)? {
            fields.insert("aspect_ratio".into(), ratio.into());
            fields.insert("resolution".into(), resolution(long, TIERS).into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_size_is_a_ratio_and_a_resolution() {
        let mut input = Input::new("p");
        input.size = Some("1152x2048".into());
        let mut fields = Map::new();
        OpenRouterImages
            .fields(&mut fields, &input)
            .expect("fields");
        assert_eq!(
            Value::Object(fields),
            json!({"aspect_ratio": "9:16", "resolution": "2K"})
        );
    }
}
