#[macro_use]

extern crate lazy_static;
extern crate ctrlc;
extern crate regex;
extern crate chrono;
extern crate clap;

use std::fmt;
use std::error;
use std::io::prelude::*;
use std::io::ErrorKind;
use std::thread;
use std::time::Duration as Timeout;
use std::collections::HashSet;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;

use log;
use env_logger;
use clap::{Arg, App};
use openssl::ssl::{SslMethod, SslConnector, SslVerifyMode, SslStream};
use rand::seq::IteratorRandom;
use regex::Regex;
use chrono::{DateTime, Utc, Duration};

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;
type IrcConnection = SslStream<TcpStream>;

mod commands;



#[derive(Debug, Clone)]
struct Error {
    msg: String,
}

impl Error {
    fn new(msg: &str) -> Error {
        Error{msg: msg.to_string()}
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        &self.msg
    }
}


fn send(s: &mut IrcConnection, msg: &String)-> Result<()> {
    let mut out = String::from(msg);
    out.push_str("\r\n");

    // log::debug!("out: {:?}", out);
    s.write(out.as_bytes())?;
    Ok(())
}

fn ident(s: &mut IrcConnection, nick: &String) -> Result<()> {
    send(s, &format!("NICK {}", nick))?;
    send(s, &format!("USER {} 0 * :{}", nick, nick))?;
    Ok(())
}

fn join(s: &mut IrcConnection, channel: &String) -> Result<()> {
    send(s, &format!("JOIN :{}", channel))?;
    Ok(())
}

pub fn say(stream: &mut IrcConnection, target: &String, what: &String) -> Result<()> {
    let mut s = what.replace("\n", "  ");
    if s.len() > 1000 {
        s = String::from(&s[..1000]);
        s.push_str("...");
    }
    let mut out = format!("PRIVMSG {} :", target);
    out.push_str(&s);
    send(stream, &out)?;
    Ok(())
}

fn quit(s: &mut IrcConnection, msg: &String) -> Result<()> {
    send(s, &format!("QUIT :{}", msg))?;
    Ok(())
}

#[derive(Debug)]
pub struct IrcPrefix {
    host: String,
    nick: String,
    realname: String,
}

fn parse_prefix(s: &String) -> IrcPrefix {
    let (host, nick, realname);

    if let Some(host_idx) = s.find("@") {
        host = String::from(&s[host_idx+1..]);

        let prefix = String::from(&s[..host_idx]);
        if let Some(prefix_idx) = prefix.find("!") {
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
pub struct IrcMessage {
    prefix: IrcPrefix,
    command: String,
    args: Vec<String>
}

fn parse_message(s: &mut String) -> IrcMessage {
    let mut prefix = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut idx = 0;

    if s.starts_with(":") {
        idx = s.find(" ").unwrap_or(s.len());
        prefix = String::from(&s[1..idx]);
    }

    if let Some(trailing_idx) = s.find(" :") {
        let trailing = String::from(&s[trailing_idx+2..]);
        let rest = String::from(&s[idx..trailing_idx]);
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
    IrcMessage {
        prefix: parse_prefix(&prefix),
        command: command,
        args: args
    }
}


fn on_welcome(bot: &mut IrcBot, stream: &mut IrcConnection, _msg: &IrcMessage) -> Result<()> {
    join(stream, &bot.channel)?;
    say(stream, &bot.channel, &String::from("high"))?;
    Ok(())
}

fn on_privmsg(bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
    if msg.args[0] != bot.channel {
        return Ok(());
    }

    let mut prefix = String::from(&bot.nick);
    prefix.push_str(": ");
    if msg.args[1].starts_with(&prefix) {
        say(stream, &bot.channel, &String::from("sup?"))?;
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
        bot.dispatch(stream, &msg, &command, &rest)?;
    } else {
        bot.see(stream, &msg)?;
    }
    Ok(())
}

fn on_ping(_bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
    let mut out = String::from("PONG :");
    out.push_str(&msg.args[0]);
    send(stream, &out)?;
    Ok(())
}

type CallbackHandler = fn(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage) -> Result<()>;
type Command = fn(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()>;


#[derive(Debug)]
pub struct SeenUrl {
    owner: String,
    count: i32,
    first: DateTime<Utc>,
}

#[derive(Debug)]
pub struct SeenNick {
    sentiment_score: f32,
    statements: i32,
}

#[derive(Debug)]
pub struct IrcBot {
    host: String,
    nick: String,
    channel: String,
    ignore: Option<Vec<String>>,
    seen_urls: HashMap<String, SeenUrl>,
    seen_nicks: HashMap<String, SeenNick>,
    last_greet: DateTime<Utc>,
}

impl IrcBot {
    fn new(host: String, nick: String, channel: String) -> IrcBot {
        IrcBot {
            host: host,
            nick: nick,
            channel: channel,
            ignore: None,
            seen_urls: HashMap::new(),
            seen_nicks: HashMap::new(),
            last_greet: Utc::now(),
        }
    }

    fn set_ignore(&mut self, ignore: Option<Vec<String>>) {
        self.ignore = ignore;
    }

    fn dispatch(&mut self, stream: &mut IrcConnection, msg: &IrcMessage, command: &String, rest: &String) -> Result<()> {
        let handler: Option<Command> = match command.as_str() {
            "weather" => Some(commands::command_weather),
            "ud" => Some(commands::command_ud),
            "giphy" => Some(commands::command_giphy),
            "strain" => Some(commands::command_strain),
            "nega" => Some(commands::command_nega),
            "image" => Some(commands::command_image),
            _ => None,
        };

        if let Some(handler_fn) = handler {
            if let Err(e) = handler_fn(self, stream, &msg, rest) {
                log::error!("command errored: {}", e);
            }
        }

        Ok(())
    }

    fn see(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
        self.check_sentiment(msg);

        self.scrape_urls(stream, msg)?;
        self.check_greeting(stream, msg)?;
        self.check_emote(stream, msg)?;

        Ok(())
    }

    fn check_greeting(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
        let greetings :HashSet<&str> = ["hi", "high", "hello", "sirs", "pals", "buddies", "friends", "amigos"].iter().cloned().collect();

        let now = Utc::now();
        let should_greet =(self.last_greet + Duration::minutes(5)) < now;
        if msg.args.len() == 2 && greetings.contains(&msg.args[1].as_str()) && should_greet {
            let target = &msg.args[0];
            let mut rng = rand::thread_rng();
            say(stream, &target, &String::from(*greetings.iter().choose(&mut rng).unwrap()))?;
            self.last_greet = now;
        }
        Ok(())
    }

    fn check_emote(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
        let re = Regex::new(r"<3\b").unwrap();
        let target = &msg.args[0];
        let text = msg.args[1..].join(" ");
        if re.is_match(&text.as_str()) {
            say(stream, &target, &String::from("❤️"))?;
        }
        Ok(())
    }

    fn check_sentiment(&mut self, msg: &IrcMessage) {
        let result = sentiment::analyze(msg.args[1].clone());

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

    fn scrape_urls(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
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
                                                      url, seen.first.to_rfc2822(), seen.owner, seen.count))?;
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
        Ok(())
    }
}


fn connect(bot: &IrcBot) -> Result<IrcConnection> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_verify(SslVerifyMode::NONE);
    let connector = builder.build();
    let tcp_stream = TcpStream::connect(&bot.host)?;
    tcp_stream.set_nodelay(true)?;
    tcp_stream.set_read_timeout(Some(Timeout::from_millis(1000)))?;
    let stream = connector.connect(&bot.host, tcp_stream)?;
    Ok(stream)
}


fn handle_message(data: String, bot: &mut IrcBot, stream: &mut IrcConnection) -> Result<()> {
    let lines = data.lines();
    for line in lines {
        if line.len() == 0 {
            break;
        }

        let msg = parse_message(&mut String::from(line));

        if let Some(ignored) = &bot.ignore {
            if ignored.iter().any(|s| s == &msg.prefix.nick) {
                log::debug!("dropped: {:?}", msg);
                return Ok(());
            }
        }

        log::debug!("incoming message: {:?}", msg);

        let handler: Option<CallbackHandler> = match msg.command.as_str() {
            "001" => Some(on_welcome),
            "PRIVMSG" => Some(on_privmsg),
            "PING" => Some(on_ping),
            _ => None,
        };

        if let Some(handler_fn) = handler {
            if let Err(e) = handler_fn(bot, stream, &msg) {
                log::error!("error handling message: {}", e);
            }
        }
    }

    Ok(())
}


fn bot_main(running: &AtomicBool, bot: &mut IrcBot, stream: &mut IrcConnection) -> Result<()> {
    ident(stream, &bot.nick)?;

    while running.load(Ordering::Relaxed) {
        let mut buffer = [0; 4096];
        match stream.read(&mut buffer) {
            Ok(bytes) => {
                let s = String::from_utf8_lossy(&buffer[0..bytes]).to_string();
                if let Err(e) = handle_message(s, bot, stream) {
                    log::error!("error handling message: {}", e);
                }
            },
            Err(e) => {
                if let ErrorKind::WouldBlock = e.kind() {
                    continue;
                }
                return Err(Box::new(e));
            }
        }
    }

    quit(stream, &String::from("out"))?;

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    let args = App::new("rusty")
        .version("0.1")
        .arg(Arg::new("host")
             .takes_value(true)
             .required(true)
             .index(1))
        .arg(Arg::new("nick")
             .takes_value(true)
             .required(true)
             .index(2))
        .arg(Arg::new("channel")
             .takes_value(true)
             .required(true)
             .index(3))
        .arg(Arg::new("ignore")
             .takes_value(true)
             .long("ignore")
             .multiple(true))
        .get_matches();

    let host = String::from(args.value_of("host").unwrap());
    let nick = String::from(args.value_of("nick").unwrap());
    let channel = String::from(args.value_of("channel").unwrap());
    let mut bot: IrcBot = IrcBot::new(host, nick, channel);

    if let Some(ignore) = args.values_of("ignore") {
        let values: Vec<String> = ignore.map(|s| s.to_string()).collect();
        bot.set_ignore(Some(values));
    }

    static RUNNING: AtomicBool = AtomicBool::new(true);
    ctrlc::set_handler(|| {
        log::info!("shutdown..");
        RUNNING.store(false, Ordering::Relaxed);
    })?;

    while RUNNING.load(Ordering::Relaxed) {
        log::info!("connecting to {}", bot.host);

        match connect(&bot) {
            Ok(mut stream) => {
                if let Err(e) = bot_main(&RUNNING, &mut bot, &mut stream) {
                    log::error!("bot errored: {}", e);
                }
                if let Err(e) = stream.shutdown() {
                    log::error!("error closing stream: {}", e);
                }
            }
            Err(e) => {
                log::error!("could not connect: {}", e);
                thread::sleep(Timeout::from_millis(5000));
            }
        }
    }

    Ok(())
}
