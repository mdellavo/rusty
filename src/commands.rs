extern crate sentiment;
extern crate image;
extern crate serde;
extern crate reqwest;
extern crate soup;
extern crate color_space;

use std::env;
use std::collections::HashMap;
use rand::seq::IteratorRandom;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, Error, say};

use soup::prelude::*;
use soup::Soup;
use serde_json::Value;
use serde::Deserialize;
use color_space::{Rgb, CompareCie2000};
use reqwest::StatusCode;
use image::imageops;

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


pub fn command_nega(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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

pub fn command_weather(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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

pub fn command_ud(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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


pub fn command_giphy(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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

pub fn command_strain(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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

#[derive(Debug, Clone)]
struct Color {
    color: Rgb,
    code: u32,
}

impl Color {
    fn new(color: u32, code: u32) -> Color {
        return Color {
            color: Rgb::from_hex(color),
            code: code,
        };
    }
}


lazy_static! {
    static ref COLORS: [Color; 99] = {
        let a = [
            Color::new(0xffffff, 0),
            Color::new(0x000000, 1),
            Color::new(0x00007f, 2),
            Color::new(0x009300, 3),
            Color::new(0xff0000, 4),
            Color::new(0x7f0000, 5),
            Color::new(0x9c009c, 6),
            Color::new(0xfc7f00, 7),
            Color::new(0xffff00, 8),
            Color::new(0x00fc00, 9),
            Color::new(0x009393, 10),
            Color::new(0x00ffff, 11),
            Color::new(0x00009c, 12),
            Color::new(0xff00ff, 13),
            Color::new(0x7f7f7f, 14),
            Color::new(0xd2d2d2, 15),
            Color::new(0x470000, 16),
            Color::new(0x472100, 17),
            Color::new(0x474700, 18),
            Color::new(0x324700, 19),
            Color::new(0x004700, 20),
            Color::new(0x00472c, 21),
            Color::new(0x004747, 22),
            Color::new(0x002747, 23),
            Color::new(0x000047, 24),
            Color::new(0x2e0047, 25),
            Color::new(0x470047, 26),
            Color::new(0x47002a, 27),
            Color::new(0x740000, 28),
            Color::new(0x743a00, 29),
            Color::new(0x747400, 30),
            Color::new(0x517400, 31),
            Color::new(0x007400, 32),
            Color::new(0x007449, 33),
            Color::new(0x007474, 34),
            Color::new(0x004074, 35),
            Color::new(0x000074, 36),
            Color::new(0x4b0074, 37),
            Color::new(0x740074, 38),
            Color::new(0x740045, 39),
            Color::new(0xb50000, 40),
            Color::new(0xb56300, 41),
            Color::new(0xb5b500, 42),
            Color::new(0x7db500, 43),
            Color::new(0x00b500, 44),
            Color::new(0x00b571, 45),
            Color::new(0x00b5b5, 46),
            Color::new(0x0063b5, 47),
            Color::new(0x0000b5, 48),
            Color::new(0x7500b5, 49),
            Color::new(0xb500b5, 50),
            Color::new(0xb5006b, 51),
            Color::new(0xff0000, 52),
            Color::new(0xff8c00, 53),
            Color::new(0xffff00, 54),
            Color::new(0xb2ff00, 55),
            Color::new(0x00ff00, 56),
            Color::new(0x00ffa0, 57),
            Color::new(0x00ffff, 58),
            Color::new(0x008cff, 59),
            Color::new(0x0000ff, 60),
            Color::new(0xa500ff, 61),
            Color::new(0xff00ff, 62),
            Color::new(0xff0098, 63),
            Color::new(0xff5959, 64),
            Color::new(0xffb459, 65),
            Color::new(0xffff71, 66),
            Color::new(0xcfff60, 67),
            Color::new(0x6fff6f, 68),
            Color::new(0x65ffc9, 69),
            Color::new(0x6dffff, 70),
            Color::new(0x59b4ff, 71),
            Color::new(0x5959ff, 72),
            Color::new(0xc459ff, 73),
            Color::new(0xff66ff, 74),
            Color::new(0xff59bc, 75),
            Color::new(0xff9c9c, 76),
            Color::new(0xffd39c, 77),
            Color::new(0xffff9c, 78),
            Color::new(0xe2ff9c, 79),
            Color::new(0x9cff9c, 80),
            Color::new(0x9cffdb, 81),
            Color::new(0x9cffff, 82),
            Color::new(0x9cd3ff, 83),
            Color::new(0x9c9cff, 84),
            Color::new(0xdc9cff, 85),
            Color::new(0xff9cff, 86),
            Color::new(0xff94d3, 87),
            Color::new(0x000000, 88),
            Color::new(0x131313, 89),
            Color::new(0x282828, 90),
            Color::new(0x363636, 91),
            Color::new(0x4d4d4d, 92),
            Color::new(0x656565, 93),
            Color::new(0x818181, 94),
            Color::new(0x9f9f9f, 95),
            Color::new(0xbcbcbc, 96),
            Color::new(0xe2e2e2, 97),
            Color::new(0xffffff, 98),
        ];
        a
    };
}

fn nearest_color(target: &Rgb) -> &Color {
    let nearest = COLORS.iter().min_by(|a, b| {
        let dist_a = a.color.compare_cie2000(target);
        let dist_b = b.color.compare_cie2000(target);
        return dist_a.partial_cmp(&dist_b).unwrap(); // FIXME
    });
    if let Some(n) = nearest {
        return &n;
    }
    return &COLORS[0];
}

pub fn command_image(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    if rest.len() < 1 {
        return Ok(());
    }

    let url = rest;
    let result = get_reqw_client().get(url).send()?;
    if result.status() != StatusCode::OK {
        let msg = format!("could not load {}: {:?}", url, result);
        return Err(Box::new(Error::new(&msg)))
    }
    let body = result.bytes()?;
    let img = image::load_from_memory(&body)?;
    let (max_width, max_height) = (20, 20);

    let thumb = imageops::resize(&img, max_width, max_height, imageops::FilterType::Lanczos3);
    let (thumb_width, thumb_height) = thumb.dimensions();

    let target = &message.args[0];

    for y in 0..thumb_height {
        let mut row = String::new();

        for x in 0..thumb_width {
            let pixel = thumb.get_pixel(x, y);
            let (r, g, b, a) = image::Pixel::channels4(pixel);
            let rgb = Rgb::new(r as f64, g as f64, b as f64);
            let nearest = nearest_color(&rgb);

            let cell = format!("\x0301,{:02} ", nearest.code);
            row.push_str(&cell);
        }
        say(stream, target, &row)?;
    }

    Ok(())
}
