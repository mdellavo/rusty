extern crate sentiment;
extern crate image;
extern crate serde;
extern crate reqwest;
extern crate soup;

use std::env;
use std::error::Error;
use std::collections::HashMap;
use rand::seq::IteratorRandom;

use crate::{IrcMessage, IrcConnection, IrcBot, say};

use soup::prelude::*;
use soup::Soup;
use serde_json::Value;
use serde::Deserialize;


fn k2f(k: f32) -> f32 {
    return k * (9./5.) - 459.67;
}


fn get_reqw_client() -> reqwest::blocking::Client {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    return client;
}


pub fn command_nega(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
    if rest.len() < 1 {
        return Ok(());
    }

    let nick = rest;
    if let Some(seen) = bot.seen_nicks.get(nick) {
        let target = &message.args[0];
        let msg = format!("avg nega for {} is {}", nick, seen.sentiment_score / seen.statements as f32);
        say(stream, &target, &msg)?;
    }
   Ok(())
}


#[derive(Deserialize, Debug)]
struct WeatherResponse {
    name: String,
    main: HashMap<String, f32>,
}

pub fn command_weather(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return Ok(());
    }

    let key = env::var("OPENWEATHER_API_KEY").unwrap();
    let url = format!("https://api.openweathermap.org/data/2.5/weather?q={},us&APPID={}", parts[0], key);
    log::debug!("GET {}", url);
    let result = get_reqw_client()
        .get(&url)
        .send()?;
    let body = result.json::<WeatherResponse>()?;
    if let Some(temp) = body.main.get("temp") {
        let feels_like = k2f(*temp);
        let msg = format!("the actual temp in {} is {:.0}f", body.name, feels_like);
        let target = &message.args[0];
        say(stream, &target, &msg)?;
    }
    Ok(())
}

#[derive(Deserialize, Debug)]
struct UdResponse {
    list: Vec<HashMap<String, Value>>,
}

pub fn command_ud(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
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


#[derive(Deserialize, Debug)]
struct GiphyResponse {
    data: Vec<HashMap<String, Value>>,
}


pub fn command_giphy(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
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

pub fn parse_strain(html: &String) -> Option<String> {
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

pub fn command_strain(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
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

pub fn command_image(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<(), Box<dyn Error>> {
    if rest.len() < 1 {
        return Ok(());
    }

    let url = rest;
    let result = get_reqw_client().get(url).send()?;
    let body = result.bytes()?;

    Ok(())
}
