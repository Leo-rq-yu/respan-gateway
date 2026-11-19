# Media formats over HTTP

`respan-gateway-media-http` is the [media kinds](../../kinds/media/README.md)
over HTTP.

`openai::{OpenAiImages, OpenAiTranscriptions, OpenAiSpeech, OpenAiVideos,
OpenAiEmbeddings}`, named `openai-images`, `openai-transcriptions`,
`openai-speech`, `openai-videos` and `openai-embeddings`: OpenAI's endpoints, as OpenAI and compatible vendors serve
them. A client's request is a `POST` to a path ending in the endpoint
(`/v1/images/generations` and `/openai/v1/images/generations` alike), or a
video job's by its method and path; a provider's base URL includes the
version, as OpenAI's SDKs take it: `https://api.openai.com/v1`,
`https://api.groq.com/openai/v1`,
`https://generativelanguage.googleapis.com/v1beta/openai`.

| Format | Endpoints | Request | Response |
| --- | --- | --- | --- |
| `openai-images` | `/images/generations`, `/images/edits` | JSON; an edit a form (`image` or `image[]`, `mask`), or JSON naming its images by `image_url` | JSON; with `stream`, `image_generation.*` or `image_edit.*` events |
| `openai-transcriptions` | `/audio/transcriptions`, `/audio/translations` | A form (`file`) | JSON (`json`, `verbose_json`, `diarized_json`) or text (`text`, `srt`, `vtt`); with `stream`, `transcript.text.*` events |
| `openai-speech` | `/audio/speech` | JSON | The audio's bytes; with `stream_format: "sse"`, `speech.audio.*` events |
| `openai-embeddings` | `/embeddings` | JSON; `input` a text, texts, token ids or lists of them | JSON; vectors as `encoding_format` asks, `float` or `base64`, whatever the provider answers |
| `openai-videos` | `POST /videos`, `GET /videos`, `GET` or `DELETE /videos/{id}`, `GET /videos/{id}/content`, `POST /videos/{id}/remix` | JSON, or a form with an `input_reference` file | A job, a list or a deletion in JSON; content as its bytes |

- A provider is asked to stream as the call's `StreamResponse` says, else
  as the client's request did, if to the same endpoint, else not. `stream`
  and `stream_format` are framing, not part of an `Input`.
- An edit goes to `/images/edits` as a form of its images' bytes, or in JSON
  if one is a URL. An image a client names by file id cannot be relayed and
  is refused.
- A file's type is its part's, unless that says none or
  `application/octet-stream`, which OpenAI refuses for images; then its
  name's, then its first bytes'.
- Text a provider rendered (`srt`, say) is kept as it came and given back as
  `text/plain`; a client that asked for text a provider gave as JSON gets
  the transcript's text, or SubRip or WebVTT of its segments.
- Audio is relayed chunk by chunk as it comes, the provider read no faster
  than the client takes it, as the content type its `response_format` names;
  a video's content likewise, as its variant's: `video/mp4`, `image/webp` for
  a thumbnail, `image/jpeg` for a spritesheet.
- A video's `seconds` is a string in OpenAI's API, as it is written; a
  client may send a number. Its `/videos/{id}` never takes
  `/videos/{id}/content` or `/remix`; an id is escaped in a call's path.
- Usage `{"type": "tokens"}` is `Usage`, `{"type": "duration"}` `Seconds`; a
  usage object's other fields, such as `input_token_details`, stay native.
- A stream that ends before its end event fails `truncated`.
- A client body without its required field (`prompt`, an edit's `image`,
  `file`, `input`) is refused, 400, before any call. A provider's error is
  read leniently, as compatible servers write it, and a client's is written
  in OpenAI's schema, `{"error": {"message", "type", "param", "code"}}`,
  under its status.

Vendors' fields the kind has no form for (Groq's `x_groq`, Together's
`steps`) stay native, so a relay in the same format keeps them.

## Vendors' dialects of OpenAI Images

`openai::DialectedImages` is OpenAI Images as a vendor serves it, in its
`ImagesDialect`: a provider format named `openai-images`, so a client of
the spec is still relayed to it. A dialect says the path of a generation,
how it takes an edit (`Edits::Spec`, or `Edits::Json` to a path, its
images' URLs, a data URL for bytes, written where it takes them), and the
call's fields as it takes them. `openai::aspect_ratio` and
`openai::resolution` turn a size, `WxH`, into the aspect ratio and the
named resolution a vendor takes; a size of a ratio it does not draw is
refused, 400, rather than dropped. The vendors' dialects
(`respan_gateway_vendors`):

| Vendor | Generation | Size | Edit |
| --- | --- | --- | --- |
| xAI | `/images/generations`, `b64_json` unless asked | `aspect_ratio` and `resolution` (`1k`, `2k`) | JSON, `image: {url, type}` or `images`; no mask |
| OpenRouter | `/images` | `aspect_ratio` and `resolution` (`512` to `4K`) | `/images`, `input_references`; no mask |
| EmpirioLabs | `/images/generations`, `n` as `num_images` | `aspect_ratio` and `resolution` (`1K` to `4K`; Grok's `1k`, `2k`); GPT Image's as it is | JSON, `image: [urls]`; no mask |
| MiniMax | the spec's | the spec's | JSON, `images: [{image_url}]`; no mask |
| Z.ai | one image, by URL | the spec's | none |

`dashscope::DashScopeImages`, named `dashscope-images`: Alibaba Cloud
Model Studio's images (Qwen-Image, Wan, Qwen-MT-Image) as DashScope's own
API serves them, at `https://dashscope-intl.aliyuncs.com/api/v1`. A
provider format alone: a client of the image kind, in any format, is
written to it.

- A model is served by its name: Qwen-Image and Wan from 2.6 at once
  (`/services/aigc/multimodal-generation/generation`, its source images
  beside the prompt in one message); Wan before 2.6 as a task of
  `/services/aigc/text2image/image-synthesis`, which takes no source
  image; `qwen-mt-image*` as a task of
  `/services/aigc/image2image/image-synthesis`, its one image a URL, its
  `target_lang` (and `source_lang`, else `auto`) native fields.
- A task is polled at `/tasks/{id}` every 2 s, for 10 minutes at most,
  within the one call: its client gets a whole answer.
- A size is written `W*H`; `negative_prompt`, `prompt_extend`, `watermark`
  and `seed` are parameters. DashScope answers by URL alone, kept 24
  hours: `b64_json`, a quality but `auto` or `standard`, or a file format
  but `png` is refused, 400, before any call. It reports images, not
  tokens: no usage is reported.

`bedrock::BedrockImages`, named `bedrock-images`: Stability AI's image
models (SD 3.5 Large, Stable Image Ultra and Core) as Amazon Bedrock's
`InvokeModel` serves them, at `https://bedrock-runtime.{region}.amazonaws.com`,
its key a Bedrock API key as a bearer token. A provider format alone.

- A call is `POST /model/{model}/invoke`. A size is the nearest of
  Stability's aspect ratios; an edit is `image-to-image` of one image's
  bytes, in base64, and takes no size; `negative_prompt`, `seed` and
  `strength` are its native fields. More than one image, a URL or a mask
  is refused, 400.
- Its `images` are base64; one a filter took (`finish_reasons` not null)
  fails the call, 400. Bedrock reports no usage.

`gemini::GeminiVeo`, named `gemini-veo`: Google's Veo as the Gemini API serves it, to clients and providers,
at `https://generativelanguage.googleapis.com/v1beta` with the key in
`x-goog-api-key`.

- A `Create` is `POST /models/{model}:predictLongRunning`, which needs a
  model: its `size` an `aspectRatio` (16:9 or 9:16, the only ones Veo makes,
  else 400) and a `resolution` (`1080p` from 1080 pixels on the short side,
  else `720p`), its `seconds` `durationSeconds` (Veo takes 4, 6 or 8, and
  refuses others itself), its reference image the instance's base64 `image`,
  and its `gemini-veo` native fields (`negativePrompt`, `personGeneration`)
  parameters.
- A job is Veo's long-running operation, its id `veo_` and the operation's
  name, `models/{model}/operations/{op}`, in unpadded base64url
  (`GeminiVeo::job_id`, `GeminiVeo::operation`), as a path segment holds
  it; another id is not found, 404. An operation not done is `in_progress`,
  done with videos `completed`, done with an error, or with none as when
  Veo's filters took them all, `failed`.
- Content is a `GET` of the URL its job names at the provider's base, its
  path after the version, so the key goes to no other host. The Gemini API
  may redirect it to storage: the transport follows only if told to
  (`HttpTransport::follow_redirects`, which says which locations), else the
  answer is a 502. A redirect to another origin leaves the key behind.
- Veo cannot remix, delete or list videos, nor make a thumbnail or
  spritesheet: those are refused, 400 `unsupported`, before any call.
- What Veo answers beyond the kind is dropped rather than kept native.
- A client of it, served by any provider of the kind: it creates with
  `POST .../models/{model}:predictLongRunning` (one instance; its prompt,
  its base64 `image`; `aspectRatio` and `resolution` read as a size,
  `durationSeconds` as seconds, its other parameters kept as `gemini-veo`
  native fields). A job is answered as an operation named
  `models/{model}/operations/{job id}`, polled at `GET .../{name}`, so a
  follow-up reaches the job's provider by its id; done, it names its video
  at the gateway, `GET .../files/{job id}:download?alt=media`, never the
  provider's own URL, which would need its key. A failure is the
  operation's `error`, or, filtered, a response of
  `raiMediaFilteredReasons`; a refusal is Google's `{"error": {code,
  message, status}}`. Veo reports no progress: an operation is done or not.
