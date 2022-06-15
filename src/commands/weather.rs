extern crate serde;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

use std::env;
use std::collections::HashMap;

use crate::utils::get_reqw_client;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct WeatherResponse {
    name: String,
    main: HashMap<String, f32>,
}

fn k2f(k: f32) -> f32 {
    return k * (9./5.) - 459.67;
}


pub fn command(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return Ok(());
    }

    let key = env::var("OPENWEATHER_API_KEY")?;
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
