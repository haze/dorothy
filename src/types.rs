use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
pub struct CompletionRequestParams {
    pub prompt: String,
    pub max_tokens: usize,
    pub temperature: Option<f64>,

    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,

    pub top_p: Option<usize>,

    #[serde(rename = "n")]
    pub choices_per_prompt: Option<usize>,

    #[serde(rename = "stop")]
    pub stop_tokens: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
enum Object {
    #[serde(rename = "text_completion")]
    TextCompletion,
}

#[derive(Deserialize, Debug)]
pub enum FinishReason {
    #[serde(rename = "length")]
    Length,
    #[serde(rename = "stop")]
    Stop,
}

impl std::default::Default for FinishReason {
    fn default() -> Self {
        FinishReason::Length
    }
}

impl std::default::Default for Object {
    fn default() -> Self {
        Object::TextCompletion
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct Choice {
    pub text: String,
    index: usize,
    log_probability: Option<f64>, // an assumption
    pub finish_reason: FinishReason,
}

/// `Completion` is the response object from a GPT3 completion api call
#[derive(Deserialize, Debug, Default)]
pub struct Completion {
    id: Option<String>,
    object: serde_json::Value,

    #[serde(rename = "created")]
    created_timestamp: u64,

    model: String,
    pub choices: Vec<Choice>,
}

/// Spectrum
pub enum Model {
    /// Most capable
    Davinci,
    Curie,
    Babbage,
    /// Lowest latency
    Ada,
}

impl Model {
    pub fn to_string(&self) -> &'static str {
        match self {
            Model::Davinci => "davinci",
            Model::Curie => "curie",
            Model::Babbage => "babbage",
            Model::Ada => "ada",
        }
    }
}
