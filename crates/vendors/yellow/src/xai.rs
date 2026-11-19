//! xAI: Chat Completions, as OpenAI serves it, and images in its dialect.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::Fault;
use respan_gateway_media::image::Input;
use respan_gateway_media_http::openai::{
    DialectedImages, Edits, ImagesDialect, aspect_ratio, resolution,
};
use serde_json::{Map, Value, json};

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.x.ai/v1";

/// xAI.
pub const VENDOR: Vendor = Vendor {
    id: "xai",
    key_env: &["XAI_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&DialectedImages::new(XaiImages), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// The aspect ratios Grok Imagine draws.
const RATIOS: &[&str] = &[
    "1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "2:1", "1:2", "19.5:9", "9:19.5", "20:9",
    "9:20",
];

/// xAI's images:
///
/// - It takes no `size`: a size is its nearest `aspect_ratio` and the
///   `resolution` (`1k`, `2k`) nearest its longer side.
/// - It answers `b64_json` unless asked otherwise: an account with zero
///   data retention cannot be given a URL.
/// - It takes an edit only in JSON, its image `{"url", "type":
///   "image_url"}` (a data URL for bytes), several as `images`, and no
///   mask.
#[derive(Debug, Clone, Copy)]
pub struct XaiImages;

impl ImagesDialect for XaiImages {
    fn edits(&self) -> Edits {
        Edits::Json {
            path: "/images/edits",
            write: write_edit,
        }
    }

    fn fields(&self, fields: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        fields.remove("size");
        if let Some((ratio, long)) = aspect_ratio(input.size.as_deref(), RATIOS)? {
            fields.insert("aspect_ratio".into(), ratio.into());
            let tier = resolution(long, &[("1k", 1024), ("2k", 2048)]);
            fields.insert("resolution".into(), tier.into());
        }
        if !fields.contains_key("response_format") {
            fields.insert("response_format".into(), "b64_json".into());
        }
        Ok(())
    }
}

/// An edit's images as xAI takes them.
fn write_edit(
    fields: &mut Map<String, Value>,
    images: Vec<String>,
    mask: Option<String>,
) -> Result<(), Fault> {
    if mask.is_some() {
        return Err(Fault::bad_request("xAI takes no mask"));
    }
    let image = |url: String| json!({ "url": url, "type": "image_url" });
    match <[String; 1]>::try_from(images) {
        Ok([one]) => fields.insert("image".into(), image(one)),
        Err(many) => fields.insert("images".into(), many.into_iter().map(image).collect()),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_an_aspect_ratio_and_a_resolution() {
        let mut input = Input::new("p");
        input.size = Some("1536x1024".into());
        let mut fields = Map::new();
        fields.insert("size".into(), "1536x1024".into());
        XaiImages.fields(&mut fields, &input).expect("fields");
        assert_eq!(
            Value::Object(fields),
            json!({"aspect_ratio": "3:2", "resolution": "1k", "response_format": "b64_json"})
        );
    }

    #[test]
    fn an_edit_names_its_image_by_url() {
        let mut fields = Map::new();
        write_edit(&mut fields, vec!["data:image/png;base64,AA".into()], None).expect("an edit");
        assert_eq!(
            fields["image"],
            json!({"url": "data:image/png;base64,AA", "type": "image_url"})
        );
        let mut fields = Map::new();
        write_edit(&mut fields, vec!["a".into(), "b".into()], None).expect("an edit");
        assert_eq!(fields["images"][1]["url"], "b");
        assert!(write_edit(&mut Map::new(), vec!["a".into()], Some("m".into())).is_err());
    }
}
