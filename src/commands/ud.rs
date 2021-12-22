extern crate serde;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

use std::collections::HashMap;

use serde_json::Value;
use serde::Deserialize;

use crate::utils::get_reqw_client;


#[derive(Deserialize, Debug)]
struct UdResponse {
    list: Vec<HashMap<String, Value>>,
}

pub fn command(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return Ok(());
    }

    let url = format!("https://api.urbandictionary.com/v0/define?term={}", parts.join("+"));
    let resp = get_reqw_client().get(&url).send()?.json::<UdResponse>()?;
    if resp.list.len() > 0 {
        if let Some(def) = resp.list[0].get("definition") {
            let target = &message.args[0];
            let value = String::from(def.as_str().unwrap());
            say(stream, &target, &value)?;
        }
    }
    Ok(())
}
