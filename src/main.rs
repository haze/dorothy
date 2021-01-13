mod api;
mod types;

use serenity::{
    async_trait,
    model::{
        channel::Message,
        gateway::Ready,
        id::{ChannelId, GuildId},
    },
    prelude::*,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;

struct HumanChatLog {
    line: String,
    name: String,
}

struct ChatHistory {
    is_private: bool,
    human_chat_log: Vec<HumanChatLog>,
    ai_chat_log: Vec<String>,
    seen_names: HashSet<String>,
    tokens_so_far: usize,
    start_context: RwLock<String>,
    configuration: Configuration,
}

impl ChatHistory {
    fn new(is_private: bool) -> Self {
        ChatHistory {
            tokens_so_far: 20, // XXX: From start context below
            seen_names: HashSet::new(),
            ai_chat_log: Vec::new(),
            human_chat_log: Vec::new(),
            start_context: RwLock::new(String::from("The following is a conversation with an AI named Dorothy. Dorothy has short, red hair, red eyes and extremely pale (almost white) skin. Dorothy appears to have a bubbly, joyful and somewhat flirtatious attitude. She often greets every patron politely and doesn't at any point seem overly aggressive or violent. She takes great pride in her work")),
            configuration: Configuration::default(),
            is_private,
        }
    }

    #[allow(dead_code)]
    fn has_logs(&self) -> bool {
        !self.human_chat_log.is_empty() || !self.ai_chat_log.is_empty()
    }

    async fn reset(&mut self) {
        self.human_chat_log.clear();
        self.ai_chat_log.clear();
        self.seen_names.clear();
        self.recalculate_tokens().await;
    }

    async fn add_human_log(&mut self, name: &str, line: &str) {
        self.calculate_new_tokens(line).await;
        self.human_chat_log.push(HumanChatLog {
            name: name.to_string(),
            line: line.to_string(),
        });
    }

    async fn add_ai_log(&mut self, line: &str) {
        self.calculate_new_tokens(line).await;
        self.ai_chat_log.push(line.to_string());
    }

    async fn continue_last_ai_log(&mut self, line: &str) {
        self.calculate_new_tokens(line).await;
        if let Some(last) = self.ai_chat_log.last_mut() {
            last.push_str(line);
        } else {
            eprintln!("Continuation with no last ai chat log!");
        }
    }

    async fn calculate_new_tokens(&mut self, line: &str) {
        let new_tokens = line.split(' ').count();
        if (new_tokens + self.tokens_so_far) > 1500 {
            self.purge_half_chat_logs();
            self.recalculate_tokens().await;
        }
        self.tokens_so_far += new_tokens;
    }

    fn purge_half_chat_logs(&mut self) {
        self.human_chat_log.drain(0..self.human_chat_log.len() / 2);
        self.ai_chat_log.drain(0..self.ai_chat_log.len() / 2);
    }

    async fn recalculate_tokens(&mut self) {
        self.tokens_so_far = self.start_context.read().await.split(' ').count();
        for human_log in &self.human_chat_log {
            self.tokens_so_far += human_log.line.split(' ').count()
                + if self.is_private {
                    1
                } else {
                    human_log.name.split(' ').count()
                };
        }
        for ai_log in &self.ai_chat_log {
            self.tokens_so_far += ai_log.split(' ').count() + 1; // XXX: bot name is assumed to be 1 char, could change in the future
        }
    }

    fn get_stop_tokens(&self, ai_name: &str) -> Vec<String> {
        let mut buf = Vec::with_capacity(2 + self.seen_names.len());
        buf.push('\n'.to_string());
        buf.push(format!("{}:", ai_name));
        if self.is_private {
            buf.push(String::from("Human:"))
        } else {
            for name in self.seen_names.iter().take(2) {
                buf.push(format!("{}:", &name));
            }
        }
        buf
    }
}

impl ChatHistory {
    async fn to_string(&self, ai_name: &str) -> String {
        use std::fmt::Write;
        let mut buf = self.start_context.read().await.to_string();
        buf.push_str("\n\n");
        let mut is_human_talking = true;

        let mut human_log_iter = self.human_chat_log.iter().fuse().peekable();
        let mut ai_log_iter = self.ai_chat_log.iter().fuse().peekable();
        while human_log_iter.peek().is_some() || ai_log_iter.peek().is_some() {
            if is_human_talking {
                if let Some(human_line) = human_log_iter.next() {
                    if let Err(why) = write!(
                        buf,
                        "{}: {}\n",
                        if self.is_private {
                            "Human"
                        } else {
                            &*human_line.name
                        },
                        human_line.line.trim(),
                    ) {
                        eprintln!("Failed to append AI Log Line to chat history: {:?}", &why);
                        break;
                    }
                } else {
                    eprintln!("no human log line found");
                }
            } else {
                if let Some(ai_line) = ai_log_iter.next() {
                    if let Err(why) = write!(
                        buf,
                        "{}: {}{}",
                        ai_name,
                        ai_line.trim(),
                        if ai_log_iter.peek().is_none() && human_log_iter.peek().is_none() {
                            " "
                        } else {
                            "\n"
                        }
                    ) {
                        eprintln!("Failed to append AI Log Line to chat history: {:?}", &why);
                        break;
                    }
                }
            }
            is_human_talking = !is_human_talking;
        }
        buf
    }
}

#[derive(Hash, Eq, PartialEq)]
enum ChatMedium {
    Channel(ChannelId),
    Guild(GuildId, ChannelId),
}

impl ChatMedium {
    fn is_channel(&self, channel_id: &ChannelId) -> bool {
        match self {
            ChatMedium::Channel(ref chan) => channel_id == chan,
            ChatMedium::Guild(_, ref chan) => channel_id == chan,
        }
    }
}

struct HistoryMap {
    history_map: Arc<RwLock<HashMap<ChatMedium, ChatHistory>>>,
}

impl std::default::Default for Configuration {
    fn default() -> Self {
        Configuration {
            top_p: Some(1),
            temperature: Some(0.9),
            frequency_penalty: Some(0.0),
            presence_penalty: Some(0.6),
        }
    }
}

struct Configuration {
    pub top_p: Option<usize>,
    pub temperature: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
}

impl Configuration {
    fn temperature_str(&self) -> String {
        self.temperature
            .map(|val| val.to_string())
            .unwrap_or_else(|| String::from("Not set"))
    }
    fn top_p_str(&self) -> String {
        self.top_p
            .map(|val| val.to_string())
            .unwrap_or_else(|| String::from("Not set"))
    }
    fn presence_penalty_str(&self) -> String {
        self.presence_penalty
            .map(|val| val.to_string())
            .unwrap_or_else(|| String::from("Not set"))
    }
    fn frequency_penalty_str(&self) -> String {
        self.frequency_penalty
            .map(|val| val.to_string())
            .unwrap_or_else(|| String::from("Not set"))
    }
}

impl HistoryMap {
    async fn contains_medium(&self, channel_id: &ChannelId) -> bool {
        let read_lock = self.history_map.read().await;
        read_lock.keys().any(|k| k.is_channel(channel_id))
    }

    async fn create_from_initial_message(&self, message: &Message) {
        let mut write_lock = self.history_map.write().await;
        write_lock.insert(
            message
                .guild_id
                .map(|guild| ChatMedium::Guild(guild, message.channel_id))
                .unwrap_or_else(|| ChatMedium::Channel(message.channel_id)),
            ChatHistory::new(message.is_private()),
        );
    }
}

impl std::default::Default for HistoryMap {
    fn default() -> Self {
        HistoryMap {
            history_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

struct Handler {
    gpt3_client: api::GPT3Client,
    history_map: HistoryMap,
    name: RwLock<Option<String>>,
}

impl Handler {
    async fn get_name(&self) -> String {
        self.name
            .read()
            .await
            .clone()
            .unwrap_or_else(|| String::from("AI"))
    }

    async fn reply(&self, ctx: &Context, message: &Message, text: &str) {
        if let Err(why) = message
            .channel_id
            .send_message(&ctx.http, |create_msg| create_msg.content(text))
            .await
        {
            eprintln!("Failed to send message: {:?}", &why);
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // don't respond to myself
        let my_id = ctx.cache.current_user_id().await;
        let is_myself = msg.author.id == my_id;
        if is_myself {
            return;
        }
        // special casing, only respond to #chat-with-ai in gamer house
        if msg.guild_id.is_some() {
            if msg.channel_id != 736764305474715650
                && msg.channel_id != 682581950971773044
                && msg.channel_id != 752799316258848820
                && msg.channel_id != 752811047479410748
                && msg.channel_id != 760421803008720938
            {
                return;
            }
        } else {
            if msg.author.id != 599131785732816898 {
                return;
            }
        }
        // if this medium doesn't exist, insert it into the map as new
        if !self.history_map.contains_medium(&msg.channel_id).await {
            self.history_map.create_from_initial_message(&msg).await;
        }
        // k cool, we can get the chat history now...
        let mut write_lock = self.history_map.history_map.write().await;
        let chat_history_ref = write_lock
            .iter_mut()
            .find(|(k, _)| k.is_channel(&msg.channel_id))
            .map(|(_, v)| v)
            .unwrap(); // this unwrap is safe, because we ensured that it existed in the map before.
        let human_content_safe_untrimmed = msg.content_safe(&ctx.cache).await.replace("\n", " ");
        let human_content_safe = human_content_safe_untrimmed.trim();
        if human_content_safe.starts_with("!") {
            eprintln!("parsing custom command");
            if msg.author.id.0 == 599131785732816898 || msg.author.id.0 == 470255953090969602 {
                if human_content_safe.starts_with("!temperature") {
                    let temp_len = "!temperature".len();
                    if temp_len == human_content_safe.len() {
                        chat_history_ref.configuration.temperature = None;
                    } else {
                        if let Ok(value) = human_content_safe
                            .chars()
                            .skip("!temperature".len() + 1)
                            .collect::<String>()
                            .parse::<f64>()
                        {
                            chat_history_ref.configuration.temperature = Some(value);
                        }
                    }
                } else if human_content_safe.starts_with("!frequency_penalty") {
                    let temp_len = "!frequency_penalty".len();
                    if temp_len == human_content_safe.len() {
                        chat_history_ref.configuration.frequency_penalty = None;
                    } else {
                        if let Ok(value) = human_content_safe
                            .chars()
                            .skip("!frequency_penalty".len() + 1)
                            .collect::<String>()
                            .parse::<f64>()
                        {
                            chat_history_ref.configuration.frequency_penalty = Some(value);
                        }
                    }
                } else if human_content_safe.starts_with("!presence_penalty") {
                    let temp_len = "!presence_penalty".len();
                    if temp_len == human_content_safe.len() {
                        chat_history_ref.configuration.presence_penalty = None;
                    } else {
                        if let Ok(value) = human_content_safe
                            .chars()
                            .skip("!presence_penalty".len() + 1)
                            .collect::<String>()
                            .parse::<f64>()
                        {
                            chat_history_ref.configuration.presence_penalty = Some(value);
                        }
                    }
                } else if human_content_safe.starts_with("!top_p") {
                    let temp_len = "!top_p".len();
                    if temp_len == human_content_safe.len() {
                        chat_history_ref.configuration.top_p = None;
                    } else {
                        if let Ok(value) = human_content_safe
                            .chars()
                            .skip("!top_p".len() + 1)
                            .collect::<String>()
                            .parse::<usize>()
                        {
                            chat_history_ref.configuration.top_p = Some(value);
                        }
                    }
                } else if human_content_safe.starts_with("!reset") {
                    chat_history_ref.reset().await;
                    self.reply(&ctx, &msg, "[Chatlog Cleared]").await;
                } else if human_content_safe.starts_with("!log") {
                    let ai_name = self.get_name().await;
                    self.reply(
                        &ctx,
                        &msg,
                        &*format!("```{}```", chat_history_ref.to_string(&*ai_name).await),
                    )
                    .await;
                } else if human_content_safe.starts_with("!context=") {
                    let mut start_context_write_lock = chat_history_ref.start_context.write().await;
                    *start_context_write_lock =
                        human_content_safe.chars().skip("#context=".len()).collect();
                    println!("updated!");
                    self.reply(
                        &ctx,
                        &msg,
                        &*format!("Context set to:\n```{}```", *start_context_write_lock,),
                    )
                    .await;
                    drop(start_context_write_lock);
                    chat_history_ref.reset().await;
                } else if human_content_safe.starts_with("!info") {
                    self.reply(&ctx, &msg, &*format!(r#"```temperature ({}): Controls randomness. Lowering results in less random completions. As the temperature approaches zero, the model will become more deterministic and repetitive.

    top_p ({}): Controls diversity via nucleus sampling. 0.5 means half of all likelihood-weighted options are considered.

    frequency_penalty ({}): How much to penalize new tokens based on their existing frequency in the text so far. Decreases the model's likelihood to repeat the same line verbatim.

    prescence_penalty ({}): How much to penalize new tokens based on whether they appear in the text so far. Increases the models liklihood to talk about new topics.

    You can set any property like this: "!top_p 2" or "!temperature 0.6"

    The current context is:
    {}
    {} tokens so far
    ```
                    "#, chat_history_ref.configuration.temperature_str(),
    chat_history_ref.configuration.top_p_str(),
    chat_history_ref.configuration.frequency_penalty_str(),
    chat_history_ref.configuration.presence_penalty_str(),
    *chat_history_ref.start_context.read().await,
    chat_history_ref.tokens_so_far,
                    )).await
                }
            }
            return;
        }

        let human_name = msg.author.name;
        let ai_name = self.get_name().await;

        if !chat_history_ref.seen_names.contains(&human_name) {
            chat_history_ref.seen_names.insert(human_name.clone());
        }

        chat_history_ref
            .add_human_log(&*human_name, human_content_safe)
            .await;

        // eprintln!("\n==== CHAT LOG SO FAR ====");
        // eprintln!("{}", guard.to_string(&*ai_name, &*start_context));
        if let Err(why) = msg.channel_id.broadcast_typing(&ctx.http).await {
            eprintln!("Could not broadcast typing: {:?}", &why);
        }

        match generate_response(&self.gpt3_client, chat_history_ref, &*ai_name).await {
            Ok(text) => {
                if let Err(why) = msg
                    .channel_id
                    .send_message(&ctx.http, |create_msg| create_msg.content(text))
                    .await
                {
                    eprintln!("Failed to send AI completion response message: {:?}", &why);
                } else {
                    eprintln!("\n==== CHAT LOG SO FAR (WITH AI) ====");
                    eprintln!("{}", chat_history_ref.to_string(&*ai_name).await);
                    eprintln!("{} tokens so far", &chat_history_ref.tokens_so_far);
                }
            }
            Err(why) => {
                eprintln!("Failed to get AI completions: {}", &why);

                if let Err(why) = msg
                    .channel_id
                        .send_message(&ctx.http, |create_msg| create_msg.content("Failed to complete, try resetting (check channel description to find out how)"))
                        .await
                {
                    eprintln!("Failed to send AI error response message: {:?}", &why);
                }
            }
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let mut guard = self.name.write().await;
        guard.replace(ready.user.name);
    }
}

async fn generate_response(
    gpt3_client: &api::GPT3Client,
    chat_history_ref: &mut ChatHistory,
    ai_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut response_buffer = String::new();
    let mut first = true;
    loop {
        let prompt = if first {
            format!("{}{}:", chat_history_ref.to_string(ai_name).await, ai_name)
        } else {
            chat_history_ref.to_string(ai_name).await
        };
        dbg!(&prompt);
        dbg!(chat_history_ref.get_stop_tokens(&*ai_name));
        let mut response = gpt3_client
            .get_completion(
                types::Model::Davinci,
                types::CompletionRequestParams {
                    // prompt: guard.get_prompt(&*ai_name, &*start_context),
                    prompt: prompt.to_string(), // ill optimize this later lol
                    presence_penalty: chat_history_ref.configuration.presence_penalty,
                    frequency_penalty: chat_history_ref.configuration.frequency_penalty,
                    temperature: chat_history_ref.configuration.temperature,
                    top_p: chat_history_ref.configuration.top_p,
                    max_tokens: 50,
                    stop_tokens: Some(chat_history_ref.get_stop_tokens(&*ai_name)),
                    choices_per_prompt: Some(1),
                },
            )
            .await?;
        dbg!(&response);
        if let Some(first_choice) = response.choices.pop() {
            let choice_text = first_choice.text.replace("\n", " ");
            if first {
                chat_history_ref.add_ai_log(&*choice_text).await;
                first = false;
            } else {
                chat_history_ref.continue_last_ai_log(&*choice_text).await;
            }
            response_buffer.push_str(&*choice_text);
            if matches!(first_choice.finish_reason, types::FinishReason::Stop) {
                break;
            }
        } else {
            break;
        }
    }
    Ok(response_buffer)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv::dotenv().ok();
    let discord_token = std::env::var("DISCORD_TOKEN").expect("Missing discord token");
    let gpt3_token = std::env::var("GPT3_TOKEN").expect("Missing discord token");
    let gpt3_client = api::GPT3Client::new(&*gpt3_token);
    let mut discord_client = Client::new(discord_token)
        .event_handler(Handler {
            gpt3_client,
            history_map: HistoryMap::default(),
            name: RwLock::new(None),
        })
        .await
        .expect("Failed to start discord client");
    if let Err(why) = discord_client.start().await {
        eprintln!("Failed to start client: {:?}", &why);
    }
    Ok(())
}
