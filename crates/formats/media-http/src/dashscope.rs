//! Alibaba Cloud Model Studio's images, as DashScope's own API serves them
//! ([`DashScopeImages`]): Qwen-Image, Wan and Qwen-MT-Image.

use std::time::Duration;

use futures::{SinkExt, StreamExt, stream};
use respan_gateway_conversation::{Media, MediaSource};
use respan_gateway_conversation_http::fields::unix_now;
use respan_gateway_conversation_http::read_provider_error;
use respan_gateway_core::{
    Connection, EventSink, EventStream, Extensions, Fault, Format, Provider,
};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::{HeaderValue, Method, header::CONTENT_TYPE};
use respan_gateway_http::{Connect, ProviderIo, Upstream, Whole};
use respan_gateway_media::image::{Image, Input, Output, Start};
use serde_json::{Map, Value, json};

use crate::edge::sends;
use crate::openai::{bytes_of, invalid, json_bytes, said_error, says_error};

const NAME: &str = "dashscope-images";

/// How often a task is polled, and for how long at most.
const POLL: Duration = Duration::from_secs(2);
const DEADLINE: Duration = Duration::from_secs(600);

/// Alibaba Cloud Model Studio's images as its own API, DashScope, serves
/// them, at `https://dashscope-intl.aliyuncs.com/api/v1` (the international
/// site), its key a bearer token. A provider format alone: no client speaks
/// it, and a request of the image kind is written to it whatever its client's
/// format.
///
/// A model is served at one of three endpoints, by its name:
///
/// - `qwen-mt-image*` translates the text in an image: a task of
///   `/services/aigc/image2image/image-synthesis`, its one image a URL, its
///   `target_lang` (and `source_lang`, else `auto`) the request's native
///   fields; its prompt is not sent.
/// - Wan before 2.6 (`wan2.5-t2i-*` and older, `wanx*`) draws only as a task
///   of `/services/aigc/text2image/image-synthesis`, and takes no source
///   image.
/// - Every other, Qwen-Image and Wan 2.6 on, answers at once at
///   `/services/aigc/multimodal-generation/generation`, its source images,
///   if any, beside the prompt in one message.
///
/// A task is polled at `/tasks/{id}` every 2 s, for 10 minutes at most,
/// within the one call: a client sees a whole answer, as from any other
/// provider of the kind.
///
/// A size is written `W*H`, as DashScope takes it, from `WxH`. Its native
/// fields `negative_prompt`, `prompt_extend`, `watermark` and `seed` are
/// parameters. DashScope answers images only by URL, kept 24 hours: a
/// request for `b64_json` is refused, 400, as is a quality but `auto` or
/// `standard`, or a file format but `png`. Usage is the images it made,
/// which the kind's usage has no place for: none is reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DashScopeImages;

impl DashScopeImages {
    /// The format's name, `dashscope-images`.
    pub const NAME: &'static str = NAME;
}

impl Upstream for DashScopeImages {
    fn name(&self) -> &str {
        NAME
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |extensions| Calls {
            connect: connect.clone(),
            extensions,
        })
    }
}

/// What makes one request's calls: its task's creation and polls.
pub struct Calls {
    connect: Connect,
    extensions: Extensions,
}

impl std::fmt::Debug for Calls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Calls").finish_non_exhaustive()
    }
}

impl Calls {
    /// One call: `method` to `path` beside the base URL, its body `body`,
    /// as a task if `task`; its answer's JSON, or the error it says.
    async fn json(
        &self,
        method: Method,
        path: String,
        body: Option<Value>,
        task: bool,
    ) -> Result<Value, Fault> {
        let io: ProviderIo = (self.connect)(self.extensions.clone());
        let bytes = match &body {
            Some(Value::Object(map)) => json_bytes(map)?,
            _ => Bytes::new(),
        };
        let (opening, mut sink) = io.with_refusal(read_provider_error).bind(
            move |head, _: Option<&Value>| {
                head.method = method;
                respan_gateway_http::path::set_path(head, &path)?;
                if body.is_some() {
                    let json = HeaderValue::from_static("application/json");
                    head.headers.insert(CONTENT_TYPE, json);
                }
                if task {
                    let enable = HeaderValue::from_static("enable");
                    head.headers.insert("x-dashscope-async", enable);
                }
                Ok(sends(bytes))
            },
            |_, _| {
                let whole = |raw: Bytes, out: &mut Vec<Value>| {
                    let v = serde_json::from_slice::<Value>(&raw).unwrap_or_default();
                    match says_error(&v) || v["code"].as_str().is_some_and(|c| !c.is_empty()) {
                        true => Err(said_error(raw)),
                        false => {
                            out.push(v);
                            Ok(())
                        }
                    }
                };
                Some(Box::new(Whole::new(whole)))
            },
        );
        sink.prepare(&mut EventStream::single(Value::Null))?;
        let (opened, closed) = futures::join!(opening, sink.close());
        let mut events = opened?;
        closed?;
        let answer = events.next().await.transpose()?;
        answer.ok_or_else(|| Fault::upstream("DashScope answered with no body"))
    }
}

impl Format<Calls, Output, Input> for DashScopeImages {
    fn name(&self) -> &str {
        NAME
    }

    fn bind(&self, calls: Calls) -> Result<Connection<Output, Input>, Fault> {
        let (given, input) = futures::channel::oneshot::channel::<Input>();
        let sink = EventSink::preparing(move |events: &mut EventStream<Input>| {
            if let Some(fault) = events.refusal() {
                return Err(fault.clone());
            }
            let streamed = || Fault::config(format!("{NAME} does not accept streaming inputs"));
            let one = events.value().transpose()?.ok_or_else(streamed)?.clone();
            events.stop();
            let _ = given.send(one);
            Ok(futures::sink::drain().sink_map_err(|never| match never {}))
        });
        let incoming = async move {
            let input = input.await.map_err(|_| Fault::abandoned())?;
            let outputs = run(&calls, &input).await?;
            Ok(EventStream::stream(stream::iter(
                outputs.into_iter().map(Ok),
            )))
        };
        Ok((Box::pin(incoming), sink))
    }
}

/// The endpoint a model is served at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    /// `multimodal-generation`, answered at once.
    Generation,
    /// `text2image`, a task.
    Synthesis,
    /// `image2image` for Qwen-MT-Image, a task.
    Translation,
}

/// The endpoint of `model`.
fn endpoint(model: &str) -> Endpoint {
    let model = model.to_ascii_lowercase();
    if model.starts_with("qwen-mt-image") {
        return Endpoint::Translation;
    }
    if model.starts_with("wanx") {
        return Endpoint::Synthesis;
    }
    // `wan2.5-t2i-preview`: its version, 2.5.
    let version = model
        .strip_prefix("wan")
        .and_then(|rest| rest.split('-').next())
        .and_then(|v| v.parse::<f64>().ok());
    match version {
        Some(v) if v < 2.6 => Endpoint::Synthesis,
        _ => Endpoint::Generation,
    }
}

/// The parameters every endpoint takes of `input`, and its refusals of what
/// DashScope cannot do.
fn parameters(input: &Input) -> Result<Map<String, Value>, Fault> {
    let native = input.native.as_ref().map(|n| &n.fields);
    let asked = |key: &str| native.and_then(|n| n.get(key)).and_then(Value::as_str);
    if asked("response_format").is_some_and(|f| f != "url") {
        return Err(invalid(
            "response_format",
            "must be url: this provider answers images only by URL",
        ));
    }
    if let Some(q) = input
        .quality
        .as_deref()
        .filter(|q| !matches!(*q, "auto" | "standard"))
    {
        return Err(invalid(
            "quality",
            &format!("{q} is not one this provider takes"),
        ));
    }
    if let Some(f) = input.output_format.as_deref().filter(|f| *f != "png") {
        return Err(invalid(
            "output_format",
            &format!("{f} is not one this provider makes"),
        ));
    }
    let mut p = Map::new();
    if let Some(size) = input.size.as_deref().filter(|s| *s != "auto") {
        let (w, h) = size
            .split_once(['x', 'X', '*'])
            .filter(|(w, h)| w.trim().parse::<u32>().is_ok() && h.trim().parse::<u32>().is_ok())
            .ok_or_else(|| invalid("size", "must be WIDTHxHEIGHT in pixels, or auto"))?;
        p.insert("size".into(), format!("{}*{}", w.trim(), h.trim()).into());
    }
    if let Some(n) = input.count {
        p.insert("n".into(), n.into());
    }
    for key in ["negative_prompt", "prompt_extend", "watermark", "seed"] {
        if let Some(v) = native.and_then(|n| n.get(key)) {
            p.insert(key.into(), v.clone());
        }
    }
    Ok(p)
}

/// The URL of `media`: its own, or a data URL of its bytes.
fn url_of(media: &Media) -> String {
    match bytes_of(media) {
        Some(bytes) => format!(
            "data:{};base64,{}",
            media.mime_type,
            crate::openai::b64(bytes)
        ),
        None => match &media.source {
            MediaSource::Url(url) => url.clone(),
            MediaSource::Bytes(_) => String::new(),
        },
    }
}

/// The body of `input` at `endpoint`.
fn body(input: &Input, model: &str, endpoint: Endpoint) -> Result<Value, Fault> {
    if input.mask.is_some() {
        return Err(invalid("mask", "is not one this provider takes"));
    }
    let parameters = parameters(input)?;
    Ok(match endpoint {
        Endpoint::Generation => {
            let mut content: Vec<Value> = input
                .images
                .iter()
                .map(|m| json!({ "image": url_of(m) }))
                .collect();
            content.push(json!({ "text": input.prompt }));
            json!({
                "model": model,
                "input": {"messages": [{"role": "user", "content": content}]},
                "parameters": parameters,
            })
        }
        Endpoint::Synthesis => {
            if input.is_edit() {
                return Err(invalid(
                    "image",
                    &format!("is not one {model} takes: it only draws"),
                ));
            }
            let mut parameters = parameters;
            let mut taken = Map::new();
            taken.insert("prompt".into(), input.prompt.clone().into());
            if let Some(negative) = parameters.remove("negative_prompt") {
                taken.insert("negative_prompt".into(), negative);
            }
            json!({"model": model, "input": taken, "parameters": parameters})
        }
        Endpoint::Translation => {
            let [image] = input.images.as_slice() else {
                return Err(invalid("image", &format!("{model} translates one image")));
            };
            let MediaSource::Url(url) = &image.source else {
                return Err(invalid(
                    "image",
                    &format!("{model} takes an image only by URL"),
                ));
            };
            let native = input.native.as_ref().map(|n| &n.fields);
            let lang = |key: &str| native.and_then(|n| n.get(key)).cloned();
            let target = lang("target_lang")
                .ok_or_else(|| invalid("target_lang", &format!("is required by {model}")))?;
            let mut taken = Map::new();
            taken.insert("image_url".into(), url.clone().into());
            taken.insert(
                "source_lang".into(),
                lang("source_lang").unwrap_or("auto".into()),
            );
            taken.insert("target_lang".into(), target);
            for key in ["ext"] {
                if let Some(v) = lang(key) {
                    taken.insert(key.into(), v);
                }
            }
            json!({"model": model, "input": taken})
        }
    })
}

/// The events of `input`, served by DashScope.
async fn run(calls: &Calls, input: &Input) -> Result<Vec<Output>, Fault> {
    let model = input
        .model
        .clone()
        .ok_or_else(|| invalid("model", &format!("is required by {NAME}")))?;
    let endpoint = endpoint(&model);
    let body = body(input, &model, endpoint)?;
    let path = match endpoint {
        Endpoint::Generation => "/services/aigc/multimodal-generation/generation",
        Endpoint::Synthesis => "/services/aigc/text2image/image-synthesis",
        Endpoint::Translation => "/services/aigc/image2image/image-synthesis",
    };
    let task = endpoint != Endpoint::Generation;
    let answer = calls
        .json(Method::POST, path.into(), Some(body), task)
        .await?;
    let done = match task {
        false => answer,
        true => poll(calls, &answer).await?,
    };
    read(&done)
}

/// A task's answer once it ended, from its creation's `created`.
async fn poll(calls: &Calls, created: &Value) -> Result<Value, Fault> {
    // A model that answers a task at once, as Qwen-MT-Image 2.0 may.
    if created["output"]["task_id"].is_null() && read(created).is_ok() {
        return Ok(created.clone());
    }
    let id = created["output"]["task_id"]
        .as_str()
        .ok_or_else(|| Fault::upstream("DashScope's task has no task_id"))?;
    let path = format!("/tasks/{}", escaped(id));
    let start = tokio::time::Instant::now();
    let mut status = created["output"]["task_status"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut answer = created.clone();
    loop {
        match status.as_str() {
            "SUCCEEDED" => return Ok(answer),
            "FAILED" | "CANCELED" | "UNKNOWN" => {
                let output = &answer["output"];
                let message = output["message"]
                    .as_str()
                    .map_or_else(|| format!("DashScope's task ended {status}"), str::to_owned);
                let code = output["code"].as_str().unwrap_or("task_failed");
                let raw = json!({"code": code, "message": message}).to_string();
                return Err(said_error(Bytes::from(raw)));
            }
            _ => {}
        }
        if start.elapsed() > DEADLINE {
            return Err(Fault::upstream(format!(
                "DashScope's task {id} did not end within {} s",
                DEADLINE.as_secs()
            )));
        }
        tokio::time::sleep(POLL).await;
        answer = calls.json(Method::GET, path.clone(), None, false).await?;
        status = answer["output"]["task_status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }
}

/// `id` as a path segment.
fn escaped(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect()
}

/// The events of an answer: a [`Start`], then an [`Image`] per image.
fn read(v: &Value) -> Result<Vec<Output>, Fault> {
    let output = &v["output"];
    let mut urls: Vec<&str> = Vec::new();
    // multimodal-generation: its choices' content.
    let contents = output["choices"].as_array().into_iter().flatten();
    let contents = contents.flat_map(|c| c["message"]["content"].as_array().into_iter().flatten());
    urls.extend(contents.filter_map(|c| c["image"].as_str()));
    // text2image: its results, but those that failed.
    let results = output["results"].as_array().into_iter().flatten();
    urls.extend(results.filter_map(|r| r["url"].as_str()));
    // image2image (Qwen-MT-Image).
    urls.extend(output["image_url"].as_str());
    if urls.is_empty() {
        return Err(Fault::upstream("DashScope's answer holds no image"));
    }
    let mut out = vec![Output::Start(Start::new(Some(unix_now())))];
    for url in urls {
        let mime = match url.split(['?', '#']).next().unwrap_or_default() {
            u if u.ends_with(".jpg") || u.ends_with(".jpeg") => "image/jpeg",
            u if u.ends_with(".webp") => "image/webp",
            _ => "image/png",
        };
        let mut image = Image::new(Media::new(mime, MediaSource::Url(url.to_owned())));
        let revised = output["results"]
            .as_array()
            .and_then(|r| r.iter().find(|r| r["url"] == url))
            .and_then(|r| r["actual_prompt"].as_str());
        image.revised_prompt = revised.map(Into::into);
        out.push(Output::Image(image));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use respan_gateway_core::{FaultKind, NativeFields};

    #[test]
    fn a_model_is_served_at_its_endpoint() {
        assert_eq!(endpoint("qwen-image-2.0-pro"), Endpoint::Generation);
        assert_eq!(endpoint("qwen-image-edit-plus"), Endpoint::Generation);
        assert_eq!(endpoint("wan2.6-t2i"), Endpoint::Generation);
        assert_eq!(endpoint("wan2.2-t2i-flash"), Endpoint::Synthesis);
        assert_eq!(endpoint("wan2.1-t2i-turbo"), Endpoint::Synthesis);
        assert_eq!(endpoint("wanx2.1-t2i-plus"), Endpoint::Synthesis);
        assert_eq!(endpoint("qwen-mt-image-2.0"), Endpoint::Translation);
    }

    #[test]
    fn a_generation_is_one_message_its_size_written_with_a_star() {
        let mut input = Input::new("a cat");
        input.size = Some("1024x1536".into());
        input.count = Some(2);
        let url = MediaSource::Url("https://x.test/a.png".into());
        input.images.push(Media::new("image/png", url));
        let mut native = Map::new();
        native.insert("negative_prompt".into(), "dogs".into());
        native.insert("background".into(), "opaque".into());
        input.native = Some(NativeFields::new("openai-images", native));
        let v = body(&input, "qwen-image-edit-plus", Endpoint::Generation).expect("a body");
        assert_eq!(
            v,
            json!({
                "model": "qwen-image-edit-plus",
                "input": {"messages": [{"role": "user", "content": [
                    {"image": "https://x.test/a.png"},
                    {"text": "a cat"},
                ]}]},
                "parameters": {"size": "1024*1536", "n": 2, "negative_prompt": "dogs"},
            })
        );
    }

    #[test]
    fn a_task_of_wan_takes_its_negative_prompt_as_input() {
        let mut input = Input::new("a cat");
        let mut native = Map::new();
        native.insert("negative_prompt".into(), "dogs".into());
        native.insert("seed".into(), 7.into());
        input.native = Some(NativeFields::new("openai-images", native));
        let v = body(&input, "wan2.2-t2i-flash", Endpoint::Synthesis).expect("a body");
        assert_eq!(
            v,
            json!({
                "model": "wan2.2-t2i-flash",
                "input": {"prompt": "a cat", "negative_prompt": "dogs"},
                "parameters": {"seed": 7},
            })
        );
    }

    #[test]
    fn what_dashscope_cannot_do_is_refused_before_a_call() {
        let refused = |edit: fn(&mut Input), model: &str| {
            let mut input = Input::new("p");
            edit(&mut input);
            body(&input, model, endpoint(model)).is_err_and(|f| f.is(FaultKind::Client))
        };
        assert!(refused(
            |i| i.quality = Some("hd".into()),
            "qwen-image-plus"
        ));
        assert!(refused(
            |i| i.output_format = Some("webp".into()),
            "qwen-image-plus"
        ));
        assert!(refused(|i| i.size = Some("big".into()), "qwen-image-plus"));
        assert!(refused(
            |i| {
                let mut native = Map::new();
                native.insert("response_format".into(), "b64_json".into());
                i.native = Some(NativeFields::new("openai-images", native));
            },
            "qwen-image-plus"
        ));
        assert!(refused(
            |i| i
                .images
                .push(Media::new("image/png", MediaSource::Url("u".into()))),
            "wan2.2-t2i-flash"
        ));
        assert!(refused(
            |i| i
                .images
                .push(Media::new("image/png", MediaSource::Url("u".into()))),
            "qwen-mt-image-2.0"
        ));
    }

    #[test]
    fn each_endpoints_answer_reads_as_its_images() {
        let generation = json!({"output": {"choices": [{"finish_reason": "stop", "message": {
            "role": "assistant", "content": [{"image": "https://oss.test/a.png?sig=1"}]}}]},
            "usage": {"image_count": 1, "width": 1024, "height": 1024}});
        let task = json!({"output": {"task_status": "SUCCEEDED", "results": [
            {"url": "https://oss.test/b.png", "actual_prompt": "a fat cat"},
            {"code": "DataInspectionFailed", "message": "x"},
        ]}, "usage": {"image_count": 1}});
        let translated = json!({"output": {"task_status": "SUCCEEDED",
            "image_url": "https://oss.test/c.jpg"}});
        let images = |v: &Value| -> Vec<(String, Option<String>)> {
            read(v)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|o| match o {
                    Output::Image(i) => match i.media.source {
                        MediaSource::Url(u) => Some((u, i.revised_prompt)),
                        MediaSource::Bytes(_) => None,
                    },
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            images(&generation),
            [("https://oss.test/a.png?sig=1".into(), None)]
        );
        assert_eq!(
            images(&task),
            [("https://oss.test/b.png".into(), Some("a fat cat".into()))]
        );
        assert_eq!(
            images(&translated),
            [("https://oss.test/c.jpg".into(), None)]
        );
        assert!(read(&json!({"output": {}})).is_err());
    }
}
