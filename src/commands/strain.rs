extern crate soup;

use soup::prelude::*;
use soup::Soup;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

use crate::utils::get_reqw_client;


fn parse_strain(html: &String) -> Option<String> {
    let soup = Soup::new(html);
    for div in soup.tag("div").find_all() {
        let class = div.get("class");
        if let None = class {
            continue
        }

        let has_description = class.unwrap().find("strain__description");
        if let None = has_description {
            continue;
        }
        return Some(div.text());
    }

    return None;
}

pub fn command(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return Ok(());
    }
    let name = parts.join("-");
    let url = format!("https://www.leafly.com/strains/{}", name);
    let body = get_reqw_client()
        .get(&url)
        .send()?
        .text()?;
    if let Some(result) = parse_strain(&body) {
        let target = &message.args[0];
        say(stream, target, &result)?;
    }
    Ok(())
}
