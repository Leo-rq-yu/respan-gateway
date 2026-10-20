//! Amazon Bedrock's InvokeModel, over HTTP: a provider format of the
//! [`embedding`](respan_gateway_media::embedding) kind, [`BedrockEmbeddings`].
//! A provider's base URL is its region's runtime,
//! `https://bedrock-runtime.{region}.amazonaws.com`, and its transport the
//! key: a Bedrock API key as a bearer token, or a signature its host adds.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::{FutureExt, Sink, SinkExt, StreamExt, TryStreamExt, stream};
use respan_gateway_conversation::Usage;
use respan_gateway_core::{
    Connection, EventSink, EventStream, Extensions, Fault, Format, Provider,
};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::HeaderValue;
use respan_gateway_http::http::header::CONTENT_TYPE;
use respan_gateway_http::http::response::Parts as ResponseHead;
use respan_gateway_http::{Connect, Decoder, Upstream, Whole};
use respan_gateway_media::embedding::{Embedding, Input, Item, Output, Start};
use serde_json::{Value, json};

use crate::edge::{bind_provider, escape, sends};
use crate::openai::OpenAiEmbeddings;

const NAME: &str = "bedrock-embeddings";
/// The header Bedrock counts a call's input tokens in.
const INPUT_TOKENS: &str = "x-amzn-bedrock-input-token-count";
/// What Cohere embeds a text as unless the client says.
const SEARCH_DOCUMENT: &str = "search_document";
/// The most calls of one request in flight: Titan's, one per text.
const IN_FLIGHT: usize = 8;

/// Amazon Bedrock's InvokeModel for embeddings, `POST
/// /model/{model}/invoke`, which needs a model: Cohere Embed
/// (`cohere.embed-*`) and Amazon Titan Embeddings
/// (`amazon.titan-embed-text-*`, `amazon.titan-embed-image-v1`), an
/// inference profile's id (`us.cohere.embed-v4:0`) as it is.
///
/// Cohere embeds every text in one call, as a `search_document` unless the
/// client's `input_type` (an [`OpenAiEmbeddings`] field) says otherwise;
/// its v4 takes `dimensions` as `output_dimension`. Titan embeds one text
/// per call, so a request of several texts is as many calls, a few at a
/// time, its vectors in the request's order and its usage theirs together;
/// Titan Text v2 takes `dimensions` as it is, Titan Multimodal as
/// `embeddingConfig.outputEmbeddingLength`. Vectors are asked for as
/// floats. Usage is the input tokens Bedrock counts: Titan's
/// `inputTextTokenCount`, else the `x-amzn-bedrock-input-token-count`
/// header, Cohere's only count.
///
/// Texts alone: token ids, parts, and a `dimensions` the model takes no
/// part of are refused, 400, before any call, as are fields of another
/// format. What Bedrock answers beyond the vectors is dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BedrockEmbeddings;

impl BedrockEmbeddings {
    /// The format's name, `bedrock-embeddings`.
    pub const NAME: &'static str = NAME;
}

/// A model family Bedrock embeds with, as its id says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Cohere { v4: bool },
    TitanText { v2: bool },
    TitanMultimodal,
}

impl Family {
    fn of(model: &str) -> Option<Family> {
        let has = |name: &str| model.contains(name);
        Some(match () {
            _ if has("cohere.embed-v4") => Family::Cohere { v4: true },
            _ if has("cohere.embed-") => Family::Cohere { v4: false },
            _ if has("amazon.titan-embed-image-") => Family::TitanMultimodal,
            _ if has("amazon.titan-embed-text-v2") => Family::TitanText { v2: true },
            _ if has("amazon.titan-embed-text-v1") || has("amazon.titan-embed-g1-text") => {
                Family::TitanText { v2: false }
            }
            _ => return None,
        })
    }
}

/// One attempt's calls: what makes each, and the attempt's extensions,
/// which each carries.
pub struct Calls {
    connect: Connect,
    extensions: Extensions,
}

impl std::fmt::Debug for Calls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Calls").finish_non_exhaustive()
    }
}

/// A bearer key, over HTTP, beside its base URL.
impl Upstream for BedrockEmbeddings {
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

impl Format<Calls, Output, Input> for BedrockEmbeddings {
    fn name(&self) -> &str {
        NAME
    }

    /// One call per body of the request, each its own I/O with the
    /// attempt's extensions: sent as the sink closes, read in order as the
    /// answer opens.
    fn bind(&self, calls: Calls) -> Result<Connection<Output, Input>, Fault> {
        let (opened, opening) = oneshot::channel::<Vec<Answer>>();
        let incoming = async move {
            let answers = opening.await.map_err(|_| Fault::abandoned())?;
            let mut out = Vec::new();
            for answer in answers {
                answer.read(&mut out).await?;
            }
            Ok(EventStream::stream(stream::iter(out.into_iter().map(Ok))))
        }
        .boxed();
        let outgoing = EventSink::preparing(move |events: &mut EventStream<Input>| {
            if let Some(fault) = events.refusal() {
                return Err(fault.clone());
            }
            if !events.is_single() {
                return Err(Fault::config(format!(
                    "{NAME} does not accept streaming inputs"
                )));
            }
            let none = || Fault::internal("a whole request has no input");
            let input = events.value().transpose()?.ok_or_else(none)?.clone();
            events.stop();
            let model = input.model.clone().unwrap_or_default();
            let (bodies, one_text) = bodies(&input)?;
            let mut sinks = Vec::new();
            let mut answers = Vec::new();
            for (at, body) in bodies.into_iter().enumerate() {
                let io = (calls.connect)(calls.extensions.clone());
                let path = format!("/model/{}/invoke", escape(&model));
                let start = Start::new(Some(model.clone()));
                let (answer, mut sink) = bind_provider(
                    io,
                    NAME,
                    move |head, _| {
                        respan_gateway_http::path::set_path(head, &path)?;
                        let json = HeaderValue::from_static("application/json");
                        head.headers.insert(CONTENT_TYPE, json);
                        Ok(sends(body))
                    },
                    move |head, _| Some(read_answer(start, head)),
                );
                sink.prepare(&mut EventStream::single(input.clone()))?;
                sinks.push(sink);
                answers.push(Answer {
                    at,
                    one_text,
                    answer,
                });
            }
            // The receiver is gone only when the answer was dropped unread.
            let _ = opened.send(answers);
            Ok(Sent::new(sinks))
        });
        Ok((incoming, outgoing))
    }
}

/// Each call's body, in order: Cohere's one, Titan's one per text; and
/// whether each is of one text.
fn bodies(input: &Input) -> Result<(Vec<Bytes>, bool), Fault> {
    let model = input.model.as_deref().unwrap_or_default();
    let family = Family::of(model).ok_or_else(|| {
        Fault::bad_request(format!(
            "a Bedrock embedding needs a Cohere Embed or Titan Embeddings model, not {model:?}"
        ))
    })?;
    let texts = (input.items.iter())
        .map(|item| match item {
            Item::Text(text) => Ok(text.as_str()),
            _ => Err(Fault::bad_request("Bedrock embeds texts alone")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let input_type = input_type(input)?;
    let no_dimensions = || Fault::bad_request(format!("Bedrock's {model} takes no `dimensions`"));
    let dimensions = input.dimensions;
    let json = |v: Value| Bytes::from(v.to_string());
    let bodies = match family {
        Family::Cohere { v4 } => {
            let mut v = json!({
                "texts": texts,
                "input_type": input_type.unwrap_or(SEARCH_DOCUMENT),
                "embedding_types": ["float"],
            });
            match (dimensions, v4) {
                (None, _) => {}
                (Some(dimensions), true) => v["output_dimension"] = dimensions.into(),
                (Some(_), false) => return Err(no_dimensions()),
            }
            vec![json(v)]
        }
        _ if input_type.is_some() => {
            return Err(Fault::bad_request("only Cohere takes an `input_type`"));
        }
        Family::TitanText { v2 } => {
            if dimensions.is_some() && !v2 {
                return Err(no_dimensions());
            }
            let body = |text: &str| match dimensions {
                Some(dimensions) => json!({"inputText": text, "dimensions": dimensions}),
                None => json!({"inputText": text}),
            };
            texts.into_iter().map(|text| json(body(text))).collect()
        }
        Family::TitanMultimodal => {
            let body = |text: &str| match dimensions {
                Some(dimensions) => json!({
                    "inputText": text,
                    "embeddingConfig": {"outputEmbeddingLength": dimensions},
                }),
                None => json!({"inputText": text}),
            };
            texts.into_iter().map(|text| json(body(text))).collect()
        }
    };
    Ok((bodies, !matches!(family, Family::Cohere { .. })))
}

/// The client's `input_type`, an [`OpenAiEmbeddings`] field. Its
/// `encoding_format` is the client edge's; any other field is refused.
fn input_type(input: &Input) -> Result<Option<&str>, Fault> {
    let Some(native) = &input.native else {
        return Ok(None);
    };
    if &*native.format != OpenAiEmbeddings::NAME {
        return Err(Fault::bad_request(format!(
            "fields of format {} cannot be written in {NAME}",
            native.format
        )));
    }
    let mut input_type = None;
    for (key, value) in &native.fields {
        match (key.as_str(), value) {
            ("encoding_format", _) => {}
            ("input_type", Value::String(value)) => input_type = Some(value.as_str()),
            _ => {
                return Err(Fault::bad_request(format!(
                    "Bedrock embeddings take no `{key}` of this kind"
                )));
            }
        }
    }
    Ok(input_type)
}

/// One call's answer, and which call it is.
struct Answer {
    at: usize,
    /// Whether its call was of one text, the request's `at`th.
    one_text: bool,
    answer: BoxFuture<'static, Result<EventStream<Output>, Fault>>,
}

impl Answer {
    /// Its events into `out`: its start only if the first call's, the
    /// vector of a call of one text as that text's.
    async fn read(self, out: &mut Vec<Output>) -> Result<(), Fault> {
        let mut events = self.answer.await?;
        while let Some(event) = events.next().await {
            match event? {
                Output::Start(_) if self.at > 0 => {}
                Output::Embedding(mut embedding) if self.one_text => {
                    embedding.index = self.at as u32;
                    out.push(Output::Embedding(embedding));
                }
                event => out.push(event),
            }
        }
        Ok(())
    }
}

/// The decoder of an answer: its vectors, Cohere's (`embeddings`, as a list
/// or by type) or Titan's (`embedding`), and its input tokens.
fn read_answer(start: Start, head: &ResponseHead) -> Box<dyn Decoder<Output>> {
    let counted = (head.headers.get(INPUT_TOKENS))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    Box::new(Whole::new(move |raw: Bytes, out: &mut Vec<Output>| {
        let v: Value = serde_json::from_slice(&raw)
            .map_err(|_| Fault::upstream("Bedrock answered no JSON"))?;
        let vectors = match (&v["embeddings"], &v["embedding"]) {
            (Value::Object(by_type), _) => by_type["float"].as_array().cloned(),
            (Value::Array(all), _) => Some(all.clone()),
            (_, one @ Value::Array(_)) => Some(vec![one.clone()]),
            _ => None,
        };
        let vectors = vectors.ok_or_else(|| Fault::upstream("Bedrock answered no vectors"))?;
        out.push(Output::Start(start.clone()));
        for (index, vector) in vectors.iter().enumerate() {
            let numbers = vector.as_array().into_iter().flatten();
            let vector = numbers
                .map(|n| n.as_f64().map(|n| n as f32))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| Fault::upstream("a Bedrock vector is not numbers"))?;
            out.push(Output::Embedding(Embedding::new(index as u32, vector)));
        }
        let tokens = v["inputTextTokenCount"].as_u64().or(counted);
        out.extend(tokens.map(|tokens| Output::Usage(Usage::new(tokens, 0))));
        Ok(())
    }))
}

/// The sink of a request's calls: each sent as it closes, a few at a time.
struct Sent {
    sinks: Option<Vec<EventSink<Input>>>,
    closing: Option<BoxFuture<'static, Result<(), Fault>>>,
}

impl Sent {
    fn new(sinks: Vec<EventSink<Input>>) -> Self {
        Self {
            sinks: Some(sinks),
            closing: None,
        }
    }
}

/// Its input was encoded as it was prepared: nothing more is sent to it.
impl Sink<Result<Input, Fault>> for Sent {
    type Error = Fault;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Fault>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, _: Result<Input, Fault>) -> Result<(), Fault> {
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Fault>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Fault>> {
        let this = self.get_mut();
        if let Some(sinks) = this.sinks.take() {
            let closes = stream::iter(sinks).map(|mut sink| async move { sink.close().await });
            this.closing = Some(closes.buffer_unordered(IN_FLIGHT).try_collect().boxed());
        }
        match &mut this.closing {
            Some(closing) => closing.poll_unpin(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}
