use crate::types;

pub struct GPT3Client {
    token: String,
}

impl GPT3Client {
    pub fn new(token: &str) -> GPT3Client {
        GPT3Client {
            token: if token.starts_with("Bearer") {
                token.to_string()
            } else {
                format!("Bearer {}", &token)
            },
        }
    }
}

impl GPT3Client {
    pub async fn get_completion(
        &self,
        model: types::Model,
        params: types::CompletionRequestParams,
    ) -> std::result::Result<types::Completion, surf::http_types::Error> {
        let client = surf::Client::new();
        let mut request = client.post(format!(
            "https://api.openai.com/v1/engines/{}/completions",
            model.to_string()
        ));
        request = request.set_header("Authorization", self.token.clone());
        request = request.body_json(&params)?;
        // let response = request.recv_string().await?;
        // Ok(match serde_json::from_str(&*response) {
        //     Ok(completion) => completion,
        //     Err(why) => {
        //         dbg!(&response);
        //         eprintln!("Failed to transmute response into json: {:?}", &why);
        //         types::Completion::default()
        //     }
        // })
        // serde_json::from_str(&response)?
        request.recv_json().await
    }
}
