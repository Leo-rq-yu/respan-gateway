# Decision formats

`respan-gateway-decision-http` is the decision kind over HTTP: the three
contracts decision models are served in, each a `POST` answered whole in
JSON. A client of any one is answered by a provider of any other.

| Format | Endpoint | Served by |
| --- | --- | --- |
| `typesafe-systemone` (`SystemOne`) | `POST /v1/systemone` | TypeSafe, Morph, Baseten, Fireworks, OpenRouter, Concentrate; at `/decisions`, Respan, DeepInfra, Perplexity, EmpirioLabs; at `/evaluations`, Prime Intellect |
| `openai-decisions` (`openai::OpenAiDecisions`) | `POST /v1/decisions` | OpenAI, Azure OpenAI, Fireworks, Concentrate |
| `gemini-decisions` (`gemini::GeminiDecisions`) | `POST /v1beta/decisions` | Gemini |

## The contracts

| | System One | OpenAI's Decisions | Gemini's Decisions |
| --- | --- | --- | --- |
| What the questions are about | `state`: a text, an object or an array | `input`: a text, or a list of messages | `input`: a text, or a `Content` of parts |
| Questions | `{id: question}` | a list, each with a `name` | `{id: question}` |
| Yes or no | `noul`, criteria `{"true", "false"}` | `predicate` | `noul` |
| One of several | `choice`, criteria `{option: description}` | `choice`, `choices: [{value, description}]` | `choice` |
| A level | `score`, criteria a list of levels | `score`, `levels: [{label, description}]` | `score` |
| Answers | `{id: answer}` | a list, in the questions' order; a question may be a `refusal` | `{id: answer}` |
| Usage | `input_tokens`, `output_tokens` | as Responses counts it, cached tokens among it | `total_input_tokens`, `total_output_tokens`, `total_tokens` |
| Key | a bearer token | a bearer token; `api-key` at Azure | `x-goog-api-key` |

A System One request is `{"model", "state", "questions": {id: question}}`,
each question `{"type", "instructions", "criteria"}`. A response is
`{"model", "answers": {id: answer}, "usage"}`: a `noul` is the probability
of yes, a `choice` has `choice`, `probabilities` by option and `confidence`,
a `score` has `score`, `probabilities` by level and `legend`.

## Between contracts

- A request's fields the kind has no form for are its format's alone, such
  as System One's `assets` or Perplexity's `mode`. They are sent to a
  provider of the same contract as they are; a provider of another is not
  sent them, and the request is refused, 400.
- An image is in each contract's own place, so it is not translated: a
  list of messages or a `Content` goes to a provider of another contract
  as JSON. Send an image in the contract of the provider that reads it.
- A state that is no text, sent to OpenAI or Gemini, is its JSON text,
  unless it is already a list of messages or a `Content`.
- A System One yes or no whose sides say what they mean is asked of OpenAI
  with them written after its instructions.
- An OpenAI question with no `name` is named by its place, `"0"` first, and
  its answer says that name.
- What a response says beyond the kind, such as System One's `legend` or
  OpenAI's `id`, only describes it: a client of the same contract is told
  it, a client of another is not.
- An answer the kind has no form for, such as OpenAI's `refusal`, is kept
  and written back where it was for a client of the same contract. It
  cannot be told a client of another, and fails the response.

A question of a type its contract does not have is refused, 400, before any
provider is called. A refusal is `{"error": {"message", "type", "code"}}`,
whose message the vendors' SDKs read.

## Vendors' dialects of System One

A vendor that serves System One at another path, or in other names, lists
its dialect (`DialectedSystemOne::new(..)`, a `SystemOneDialect`), a format
still named `typesafe-systemone`:

| Vendor | Dialect |
| --- | --- |
| Respan, Perplexity, EmpirioLabs | `At("/decisions")`: System One as it is, at `/decisions`. |
| DeepInfra | At `/decisions`. More than 64 questions, or a choice of more than 52 options, is refused, 400, before the call. |
| Prime Intellect | At `/evaluations`. A `noul` is a `boolean`, asked and answered; its usage counts are `inputTokens` and `outputTokens`. More than 64 questions are refused before the call. |
