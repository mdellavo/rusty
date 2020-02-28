#[macro_use]

extern crate serde;
extern crate ctrlc;
extern crate reqwest;
extern crate soup;
extern crate regex;
extern crate chrono;
extern crate sentiment;

use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::io::prelude::*;
use std::io::ErrorKind;
use std::collections::HashSet;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::time::Duration as Timeout;

use openssl::ssl::{SslMethod, SslConnector, SslVerifyMode, SslStream};
use serde_json::Value;
use rand::seq::IteratorRandom;
use soup::prelude::*;
use soup::Soup;
use regex::Regex;
use chrono::{DateTime, Utc, Duration};

type IrcConnection = SslStream<TcpStream>;


fn send(s: &mut IrcConnection, msg: &String) {
    let mut out = String::from(msg);
    println!(">>> {}", out);
    out.push_str("\r\n");
    let result = s.write(out.as_bytes());
    match result {
        Ok(_) => {

        },
        Err(e) => {
            println!("error: {}", e);
        }
    }
}

fn ident(s: &mut IrcConnection, nick: &String) {
    send(s, &format!("NICK {}", nick));
    send(s, &format!("USER {} 0 * :{}", nick, nick));
}

fn join(s: &mut IrcConnection, channel: &String) {
    send(s, &format!("JOIN :{}", channel))
}

fn say(stream: &mut IrcConnection, target: &String, what: &String) {
    let mut s = what.replace("\n", "  ");
    if s.len() > 400 {
        s = String::from(&s[..400]);
        s.push_str("...");
    }
    send(stream, &format!("PRIVMSG {} :{}", target, &s));
}

fn quit(s: &mut IrcConnection, msg: &String) {
    send(s, &format!("QUIT :{}", msg));
}

#[derive(Debug)]
struct IrcPrefix {
    host: String,
    nick: String,
    realname: String,
}

fn parse_prefix(s: &String) -> IrcPrefix {
    let (host, nick, realname);

    let hostsep = s.find("@");
    if hostsep.is_some() {
        let host_idx = hostsep.unwrap();
        host = String::from(&s[host_idx+1..]);

        let prefix = String::from(&s[..host_idx]);
        let prefixsep = prefix.find("!");
        if prefixsep.is_some() {
            let prefix_idx = prefixsep.unwrap();
            nick = String::from(&prefix[..prefix_idx]);
            realname = String::from(&prefix[prefix_idx+1..]);
        } else {
            nick = prefix;
            realname = String::from("");
        }
    } else {
        nick = String::from("");
        realname = String::from("");
        host = s.to_string();
    }

    IrcPrefix {
        host: host,
        nick: nick,
        realname: realname
    }
}

#[derive(Debug)]
struct IrcMessage {
    prefix: IrcPrefix,
    command: String,
    args: Vec<String>
}

fn parse_message(s: &mut String) -> IrcMessage {
    let mut prefix = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut idx = 0;

    if s.starts_with(":") {
        idx = s.find(" ").unwrap();
        prefix = String::from(&s[1..idx]);
    }

    let trailing = s.find(" :");
    if trailing.is_some() {
        let trailin_idx = trailing.unwrap();

        let trailing = String::from(&s[trailin_idx+2..]);
        let rest = String::from(&s[idx..trailin_idx]);
        for part in rest.split_whitespace() {
            args.push(String::from(part));
        }
        args.push(trailing)

    } else {
        let rest = String::from(&s[idx..]);
        for part in rest.split_whitespace() {
            args.push(String::from(part));
        }
    }

    let command = args.remove(0);
    IrcMessage{
        prefix: parse_prefix(&prefix),
        command: command,
        args: args
    }
}


fn on_welcome(bot: &mut IrcBot, stream: &mut IrcConnection, _msg: &IrcMessage) {
    join(stream, &bot.channel);
    say(stream, &bot.channel, &String::from("high"));
}

fn on_privmsg(bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) {
    if msg.args[0] != bot.channel {
        return;
    }

    let mut prefix = String::from(&bot.nick);
    prefix.push_str(": ");
    if msg.args[1].starts_with(&prefix) {
        say(stream, &bot.channel, &String::from("sup?"))
    } else if msg.args[1].starts_with("!") {
        let end = msg.args[1].find(" ");
        let end_idx;
        if end.is_some() {
            end_idx = end.unwrap();
        } else {
            end_idx = msg.args[1].len()
        }

        let command = String::from(&msg.args[1][1..end_idx]);
        let rest;
        if msg.args[1].len() > end_idx + 1 {
            rest = String::from(msg.args[1][end_idx+1..].trim());
        } else {
            rest = String::from("");
        }
        println!("command: {} / rest: {}", command, rest);
        bot.dispatch(stream, &msg, &command, &rest);
    } else {
        bot.see(stream, &msg);
    }
}

fn on_ping(_bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) {
    let mut out = String::from("PONG :");
    out.push_str(&msg.args[0]);
    send(stream, &out);
}

type CallbackHandler = fn(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage);
type Command = fn(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String);


#[derive(Debug)]
struct SeenUrl {
    owner: String,
    count: i32,
    first: DateTime<Utc>,
}

#[derive(Debug)]
struct SeenNick {
    sentiment_score: f32,
    statements: i32,
}

#[derive(Debug)]
struct IrcBot {
    host: String,
    nick: String,
    channel: String,
    seen_urls: HashMap<String, SeenUrl>,
    seen_nicks: HashMap<String, SeenNick>,
    last_greet: DateTime<Utc>
}

impl IrcBot {
    fn new(host: String, nick: String, channel: String) -> IrcBot {
        IrcBot {
            host: host,
            nick: nick,
            channel: channel,
            seen_urls: HashMap::new(),
            seen_nicks: HashMap::new(),
            last_greet: Utc::now(),
        }
    }

    fn dispatch(&mut self, stream: &mut IrcConnection, msg: &IrcMessage, command: &String, rest: &String) {
        let handler: Option<Command> = match command.as_str() {
            "weather" => Some(command_weather),
            "ud" => Some(command_ud),
            "giphy" => Some(command_giphy),
            "strain" => Some(command_strain),
            "nega" => Some(command_nega),
            _ => None,
        };
        if handler.is_some() {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                handler.unwrap()(self, stream, &msg, rest);
            }));
            if result.is_err() {
                println!("error: {:?}", result.unwrap());
            }
        }
    }

    fn see(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) {
        self.check_greeting(stream, msg);
        self.scrape_urls(stream, msg);
        self.check_emote(stream, msg);
        self.check_sentiment(stream, msg);
    }

    fn check_greeting(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) {
        let greetings :HashSet<&str> = ["hi", "high", "hello", "sirs", "pals", "buddies", "friends", "amigos"].iter().cloned().collect();

        let now = Utc::now();
        let should_greet =(self.last_greet + Duration::minutes(5)) < now;
        if msg.args.len() == 2 && greetings.contains(&msg.args[1].as_str()) && should_greet {
            let target = &msg.args[0];
            let mut rng = rand::thread_rng();
            say(stream, &target, &String::from(*greetings.iter().choose(&mut rng).unwrap()));
            self.last_greet = now;
        }
    }

    fn check_emote(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) {
        let re = Regex::new(r"<3\b").unwrap();
        let target = &msg.args[0];
        let text = msg.args[1..].join(" ");
        if re.is_match(&text.as_str()) {
            say(stream, &target, &String::from("❤️"));
        }
    }

    fn check_sentiment(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) {
        let result = sentiment::analyze(msg.args[1].clone());
        let target = &msg.args[0];

        match self.seen_nicks.get_mut(&msg.prefix.nick) {
            Some(seen) => {
                seen.sentiment_score += result.score;
                seen.statements += 1;
            },
            None => {
                self.seen_nicks.insert(msg.prefix.nick.clone(), SeenNick {
                    sentiment_score: result.score,
                    statements: 1,
                });
            }
        }
    }

    fn scrape_urls(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) {
        let re = Regex::new(r"http[s]?://(?:[a-zA-Z]|[0-9]|[$-_@.&+]|[!*(),]|(?:%[0-9a-fA-F][0-9a-fA-F]))+").unwrap();
        let text = msg.args[1..].join(" ");
        for found in re.find_iter(&text) {
            let url = &text[found.start()..found.end()];
            match self.seen_urls.get_mut(url) {
                Some(seen) => {
                    if msg.prefix.nick != seen.owner {
                        seen.count += 1;
                        let target = &msg.args[0];
                        say(stream, &target, &format!("repost: {} (first seen at {} by {} / repost count: {})",
                                                      url, seen.first.to_rfc2822(), seen.owner, seen.count));
                    }
                },
                None => {
                    self.seen_urls.insert(url.to_string(), SeenUrl {
                        owner: String::from(&msg.prefix.nick),
                        count: 1,
                        first: Utc::now()
                    });
                }
            }
        }
    }
}

fn connect(bot: &IrcBot) -> IrcConnection {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = builder.build();
    let tcp_stream = TcpStream::connect(&bot.host).unwrap();
    tcp_stream.set_nodelay(true).expect("could not set nodelay");
    tcp_stream.set_read_timeout(Some(Timeout::new(1, 0))).expect("could not set timeout");
    let stream = connector.connect(&bot.host, tcp_stream).unwrap();
    return stream;
}

#[derive(Deserialize, Debug)]
struct WeatherResponse {
    name: String,
    main: HashMap<String, f32>,
}

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

fn command_nega(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) {
    if rest.len() < 1 {
        return;
    }

    let nick = rest;
    match bot.seen_nicks.get(nick) {
        Some(seen) => {
            let target = &message.args[0];
            let msg = format!("avg nega for {} is {}", nick, seen.sentiment_score / seen.statements as f32);
            say(stream, &target, &msg)
        },
        None => {},
    }
}


fn command_weather(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return;
    }

    let key = env::var("OPENWEATHER_API_KEY").unwrap();
    let url = format!("https://api.openweathermap.org/data/2.5/weather?q={},us&APPID={}", parts[0], key);
    println!("weather: {}", url);
    let result = get_reqw_client()
        .get(&url)
        .send();
    if result.is_err() {
        return;
    }
    let body = result.unwrap().json::<WeatherResponse>();
    match body {
        Ok(resp) => {
            let feels_like = k2f(*resp.main.get("temp").unwrap());
            let msg = format!("the actual temp in {} is {:.0}f", resp.name, feels_like);
            let target = &message.args[0];
            say(stream, &target, &msg);
        }
        Err(e) => {
            println!("error: {}", e);
       }
    }
}

#[derive(Deserialize, Debug)]
struct UdResponse {
    list: Vec<HashMap<String, Value>>,
}

fn command_ud(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return;
    }

    let url = format!("https://api.urbandictionary.com/v0/define?term={}", parts.join("+"));
    let result = get_reqw_client().get(&url).send();
    if result.is_err() {
        return;
    }

    let body = result.unwrap().json::<UdResponse>();
    match body {
        Ok(resp) => {
            if resp.list.len() > 0 {
                let def = resp.list[0].get("definition");
                let msg = String::from(def.unwrap().as_str().unwrap());
                let target = &message.args[0];
                say(stream, &target, &msg);
            }
        }
        Err(e) => {
            println!("error: {}", e);
        }
    }
}


#[derive(Deserialize, Debug)]
struct GiphyResponse {
    data: Vec<HashMap<String, Value>>,
}


fn command_giphy(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return;
    }
    let key = env::var("GIPHY_API_KEY").unwrap();
    let url = format!("https://api.giphy.com/v1/gifs/search?api_key={}&q={}&rating=r", key, parts.join("+"));
    let result = get_reqw_client()
        .get(&url)
        .send();
    if result.is_err() {
        return;
    }

    let body = result.unwrap().json::<GiphyResponse>();

    match body {
        Ok(resp) => {
            let mut rng = rand::thread_rng();
            if resp.data.len() == 0 {
                return;
            }
            let item =  resp.data.iter().choose(&mut rng).unwrap();
            let img = item.get("url");
            let msg = String::from(img.unwrap().as_str().unwrap());
            let target = &message.args[0];
            say(stream, &target, &msg);
        }
        Err(e) => {
            println!("error: {}", e);
        }
    }
}

fn parse_strain(html: &String) -> Option<String> {
    let soup = Soup::new(html);
    for div in soup.tag("div").find_all() {
        let class = div.get("class");
        if class.is_none() {
            continue;
        }

        let has_description = class.unwrap().find("strain__description");
        if has_description.is_none() {
            continue;
        }
        return Some(div.text());
    }

    return None;
}

fn command_strain(_bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return;
    }
    let name = parts.join("-");
    let url = format!("https://www.leafly.com/strains/{}", name);
    let result = get_reqw_client()
        .get(&url)
        .send();

    if result.is_err() {
        return;
    }
    let body = result.unwrap().text();

    match body {
        Ok(resp) => {
            let result = parse_strain(&resp);
            if result.is_some() {
                let target = &message.args[0];
                say(stream, target, &result.unwrap());
            }
        }
        Err(e) => {
            println!("error: {}", e);
        }
    }
}

fn main() {

    let args: Vec<String> = env::args().collect();
    let mut bot: IrcBot = IrcBot::new(String::from(&args[1]), String::from(&args[2]), String::from(&args[3]));

    let mut stream = connect(&bot);
    static RUNNING: AtomicBool = AtomicBool::new(true);
    ctrlc::set_handler(|| {
        println!("shutdown..");
        RUNNING.store(false, Ordering::Relaxed);
    }).expect("could not set signal handler");

    ident(&mut stream, &bot.nick);

    while RUNNING.load(Ordering::Relaxed) {
        let mut buffer = [0; 4096];
        let result = stream.read(&mut buffer);
        match result {
            Ok(bytes) => {
                let data = String::from_utf8_lossy(&buffer[0..bytes]);
                let lines = data.lines();
                for line in lines {
                    if line.len() == 0 {
                        break;
                    }
                    let msg = parse_message(&mut String::from(line));
                    println!("{:?}", msg);

                    let handler: Option<CallbackHandler> = match msg.command.as_str() {
                        "001" => Some(on_welcome),
                        "PRIVMSG" => Some(on_privmsg),
                        "PING" => Some(on_ping),
                        _ => None,
                    };
                    if handler.is_some() {
                        let handler_fn = handler.unwrap();
                        handler_fn(&mut bot, &mut stream, &msg);
                    }
                }
            },
            Err(e) => {
                match e.kind() {
                    ErrorKind::WouldBlock => {
                        continue;
                    },
                    _ => {
                        println!("error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    quit(&mut stream, &String::from("out"));
    stream.shutdown().expect("error shutting down");
}
