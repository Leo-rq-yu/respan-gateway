//! Stability AI's image models as Amazon Bedrock serves them
//! ([`BedrockImages`]).

use respan_gateway_conversation::{Media, MediaSource};
use respan_gateway_conversation_http::fields::unix_now;
use respan_gateway_core::{Connection, Fault, Format, Provider};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::request::Parts as RequestHead;
use respan_gateway_http::{Connect, ProviderIo, Upstream, Whole};
use respan_gateway_media::image::{Image, Input, Output, Start};
use serde_json::{Map, Value};

use crate::edge::{bind_provider, sends};
use crate::openai::{aspect_ratio, b64, invalid, json_bytes, said_error, says_error, unb64};

const NAME: &str = "bedrock-images";

/// The aspect ratios Stability's models on Bedrock draw.
const RATIOS: &[&str] = &[
    "16:9", "1:1", "21:9", "2:3", "3:2", "4:5", "5:4", "9:16", "9:21",
];

/// Stability AI's image models (Stable Diffusion 3.5 Large, Stable Image
/// Ultra and Core) as Amazon Bedrock's `InvokeModel` serves them, at
/// `https://bedrock-runtime.{region}.amazonaws.com`, its key a Bedrock API
/// key as a bearer token. A provider format alone: a request of the image
/// kind is written to it whatever its client's format.
///
/// A call is `POST /model/{model}/invoke`, which needs a model. Its body is
/// the prompt, its size as the nearest of Stability's aspect ratios (another
/// is refused, 400), its file format, and its native fields
/// `negative_prompt`, `seed` and `strength`; an edit, of one image's bytes,
/// is `image-to-image`, its image in base64, and takes no size. Stability
/// draws one image a call: more is refused, 400, before any call.
///
/// The answer's `images` are base64; an image a filter took, its
/// `finish_reasons` not null, fails the call. Bedrock reports no usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BedrockImages;

impl BedrockImages {
    /// The format's name, `bedrock-images`.
    pub const NAME: &'static str = NAME;
}

impl Upstream for BedrockImages {
    fn name(&self) -> &str {
        NAME
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |x| connect(x))
    }
}

impl Format<ProviderIo, Output, Input> for BedrockImages {
    fn name(&self) -> &str {
        NAME
    }

    fn bind(&self, io: ProviderIo) -> Result<Connection<Output, Input>, Fault> {
        Ok(bind_provider(
            io,
            NAME,
            |head: &mut RequestHead, input: &Input| {
                let (path, body) = call(input)?;
                respan_gateway_http::path::set_path(head, &path)?;
                let json = respan_gateway_http::http::HeaderValue::from_static("application/json");
                head.headers.insert(
                    respan_gateway_http::http::header::CONTENT_TYPE,
                    json.clone(),
                );
                head.headers
                    .insert(respan_gateway_http::http::header::ACCEPT, json);
                Ok(sends(json_bytes(&body)?))
            },
            |_, _| Some(Box::new(Whole::new(decode))),
        ))
    }
}

/// A model's id as a path segment: `:` and `/` escaped.
fn segment(model: &str) -> String {
    let mut out = String::new();
    for c in model.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            c => {
                let mut buf = [0; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out += &format!("%{b:02X}");
                }
            }
        }
    }
    out
}

/// The path and body of `input`'s call.
fn call(input: &Input) -> Result<(String, Map<String, Value>), Fault> {
    let model = input
        .model
        .as_deref()
        .ok_or_else(|| invalid("model", &format!("is required by {NAME}")))?;
    if input.count.is_some_and(|n| n > 1) {
        return Err(invalid(
            "n",
            "must be 1: this provider draws one image a call",
        ));
    }
    if input.mask.is_some() {
        return Err(invalid("mask", "is not one this provider takes"));
    }
    let native = input.native.as_ref().map(|n| &n.fields);
    if native
        .and_then(|n| n.get("response_format"))
        .and_then(Value::as_str)
        .is_some_and(|f| f != "b64_json")
    {
        return Err(invalid(
            "response_format",
            "must be b64_json: this provider answers images only in base64",
        ));
    }
    let mut body = Map::new();
    body.insert("prompt".into(), input.prompt.clone().into());
    match input.images.as_slice() {
        [] => {
            if let Some((ratio, _)) = aspect_ratio(input.size.as_deref(), RATIOS)? {
                body.insert("aspect_ratio".into(), ratio.into());
            }
        }
        [image] => {
            let MediaSource::Bytes(bytes) = &image.source else {
                return Err(invalid(
                    "image",
                    "must be a file: this provider takes no URL",
                ));
            };
            if input.size.as_deref().is_some_and(|s| s != "auto") {
                return Err(invalid(
                    "size",
                    "is not taken by an edit: it keeps its image's",
                ));
            }
            body.insert("mode".into(), "image-to-image".into());
            body.insert("image".into(), b64(bytes).into());
        }
        _ => {
            return Err(invalid(
                "image",
                "must be one image: this provider edits one",
            ));
        }
    }
    if let Some(format) = input.output_format.as_deref() {
        let format = match format {
            "jpg" => "jpeg",
            f => f,
        };
        body.insert("output_format".into(), format.into());
    }
    for key in ["negative_prompt", "seed", "strength"] {
        if let Some(v) = native.and_then(|n| n.get(key)) {
            body.insert(key.into(), v.clone());
        }
    }
    Ok((format!("/model/{}/invoke", segment(model)), body))
}

/// The events of Bedrock's answer `raw`.
fn decode(raw: Bytes, out: &mut Vec<Output>) -> Result<(), Fault> {
    let v = serde_json::from_slice::<Value>(&raw).unwrap_or_default();
    let Some(images) = v["images"].as_array() else {
        return Err(match says_error(&v) || v["message"].is_string() {
            true => said_error(raw),
            false => Fault::upstream(format!(
                "the provider's success is not a response of {NAME}"
            )),
        });
    };
    let reasons = v["finish_reasons"].as_array();
    if let Some(reason) = reasons
        .into_iter()
        .flatten()
        .find_map(|r| r.as_str().filter(|r| !r.is_empty()))
    {
        return Err(Fault::bad_request(format!(
            "the provider made no image: {reason}"
        )));
    }
    out.push(Output::Start(Start::new(Some(unix_now()))));
    for image in images {
        let bytes = image
            .as_str()
            .and_then(unb64)
            .ok_or_else(|| Fault::upstream(format!("the provider's {NAME} image is not base64")))?;
        let mime = match bytes.first() {
            Some(0xFF) => "image/jpeg",
            Some(b'R') => "image/webp",
            _ => "image/png",
        };
        out.push(Output::Image(Image::new(Media::new(
            mime,
            MediaSource::Bytes(bytes),
        ))));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use respan_gateway_core::{FaultKind, NativeFields};
    use serde_json::json;

    #[test]
    fn a_generation_takes_its_size_as_an_aspect_ratio() {
        let mut input = Input::new("a cat");
        input.model = Some("stability.sd3-5-large-v1:0".into());
        input.size = Some("1536x1024".into());
        input.output_format = Some("jpg".into());
        let mut native = Map::new();
        native.insert("seed".into(), 7.into());
        input.native = Some(NativeFields::new("openai-images", native));
        let (path, body) = call(&input).expect("a call");
        assert_eq!(path, "/model/stability.sd3-5-large-v1%3A0/invoke");
        assert_eq!(
            Value::Object(body),
            json!({"prompt": "a cat", "aspect_ratio": "3:2", "output_format": "jpeg", "seed": 7})
        );
    }

    #[test]
    fn an_edit_is_image_to_image_of_its_bytes() {
        let mut input = Input::new("make it blue");
        input.model = Some("stability.sd3-5-large-v1:0".into());
        let bytes = MediaSource::Bytes(Bytes::from_static(b"\x89PNG"));
        input.images.push(Media::new("image/png", bytes));
        let (_, body) = call(&input).expect("a call");
        assert_eq!(body["mode"], "image-to-image");
        assert_eq!(body["image"], "iVBORw==");
    }

    #[test]
    fn what_stability_cannot_draw_is_refused() {
        let refused = |edit: fn(&mut Input)| {
            let mut input = Input::new("p");
            input.model = Some("m".into());
            edit(&mut input);
            call(&input).is_err_and(|f| f.is(FaultKind::Client))
        };
        assert!(refused(|i| i.count = Some(2)));
        assert!(refused(|i| i.size = Some("1000x100".into())));
        assert!(refused(|i| i.model = None));
        assert!(refused(|i| {
            let url = MediaSource::Url("https://x.test/a.png".into());
            i.images.push(Media::new("image/png", url));
        }));
    }

    #[test]
    fn an_answer_is_its_images_and_a_filtered_one_fails() {
        let mut out = Vec::new();
        let raw = json!({"seeds": [1], "finish_reasons": [null], "images": ["iVBORw=="]});
        decode(Bytes::from(raw.to_string()), &mut out).expect("images");
        assert!(matches!(&out[1], Output::Image(i) if i.media.mime_type == "image/png"));
        let raw = json!({"seeds": [1], "finish_reasons": ["Filter reason: prompt"], "images": []});
        assert!(decode(Bytes::from(raw.to_string()), &mut Vec::new()).is_err());
    }
}
