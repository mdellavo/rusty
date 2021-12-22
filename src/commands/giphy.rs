extern crate serde;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

use std::env;
use std::collections::HashMap;

use rand::seq::IteratorRandom;

use serde_json::Value;
use serde::Deserialize;

use crate::utils::get_reqw_client;


#[derive(Deserialize, Debug)]
struct GiphyResponse {
    data: Vec<HashMap<String, Value>>,
}


pub fn command(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return Ok(());
    }
    let key = env::var("GIPHY_API_KEY").unwrap();
    let url = format!("https://api.giphy.com/v1/gifs/search?api_key={}&q={}&rating=r", key, parts.join("+"));
    let body = get_reqw_client()
        .get(&url)
        .send()?
        .json::<GiphyResponse>()?;

    let mut rng = rand::thread_rng();
    if body.data.len() > 0 {
        if let Some(item) = body.data.iter().choose(&mut rng) {
            if let Some(url) = item.get("url") {
                let value = String::from(url.as_str().unwrap());
                let target = &message.args[0];
                say(stream, &target, &value)?;
            }
        }
    }
    Ok(())
}
