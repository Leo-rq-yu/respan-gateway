//! EmpirioLabs: images in its dialect of OpenAI's, and decisions as System
//! One, at `/decisions`.

use respan_gateway_core::Fault;
use respan_gateway_decision_http::{At, DialectedSystemOne};
use respan_gateway_media::image::Input;
use respan_gateway_media_http::openai::{
    DialectedImages, Edits, ImagesDialect, aspect_ratio, resolution,
};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.empiriolabs.ai/v1";

/// EmpirioLabs.
pub const VENDOR: Vendor = Vendor {
    id: "empiriolabs",
    key_env: &["EMPIRIOLABS_API_KEY"],
    formats: &[
        (&DialectedImages::new(EmpirioImages), Some(BASE_URL)),
        (&DialectedSystemOne::new(At("/decisions")), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// The aspect ratios its models name.
const RATIOS: &[&str] = &[
    "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9", "9:21", "1:2", "2:1",
];

/// EmpirioLabs' images, which most of its models take as they do Grok's
/// and Wan's:
///
/// - A size is its nearest `aspect_ratio` and `resolution`, `1K`, `2K` or
///   `4K` (Grok's `1k` or `2k`); a GPT Image model takes its size as it is.
/// - A count is `num_images`.
/// - An edit is JSON, its images `image`, URLs (a data URL for bytes), and
///   takes no mask.
#[derive(Debug, Clone, Copy)]
pub struct EmpirioImages;

impl ImagesDialect for EmpirioImages {
    fn edits(&self) -> Edits {
        Edits::Json {
            path: "/images/edits",
            write: |fields, images, mask| {
                if mask.is_some() {
                    return Err(Fault::bad_request("EmpirioLabs takes no mask"));
                }
                fields.insert("image".into(), images.into());
                Ok(())
            },
        }
    }

    fn fields(&self, fields: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        let model = input
            .model
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(n) = fields.remove("n") {
            fields.insert("num_images".into(), n);
        }
        if model.contains("gpt-image") {
            return Ok(());
        }
        fields.remove("size");
        if let Some((ratio, long)) = aspect_ratio(input.size.as_deref(), RATIOS)? {
            fields.insert("aspect_ratio".into(), ratio.into());
            let tier = match model.contains("grok") {
                true => resolution(long, &[("1k", 1024), ("2k", 2048)]),
                false => resolution(long, &[("1K", 1024), ("2K", 2048), ("4K", 4096)]),
            };
            fields.insert("resolution".into(), tier.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_size_and_a_count_are_as_it_names_them() {
        let mut input = Input::new("p");
        input.model = Some("wan2.7-image".into());
        input.size = Some("2048x2048".into());
        let mut fields = Map::new();
        fields.insert("n".into(), 2.into());
        fields.insert("size".into(), "2048x2048".into());
        EmpirioImages.fields(&mut fields, &input).expect("fields");
        assert_eq!(
            Value::Object(fields),
            json!({"num_images": 2, "aspect_ratio": "1:1", "resolution": "2K"})
        );
        input.model = Some("gpt-image-2".into());
        let mut fields = Map::new();
        fields.insert("size".into(), "2048x2048".into());
        EmpirioImages.fields(&mut fields, &input).expect("fields");
        assert_eq!(fields["size"], "2048x2048");
    }
}
