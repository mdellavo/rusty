#[macro_use]

extern crate serde;
extern crate ctrlc;
extern crate reqwest;

use std::env;
use std::io::prelude::*;
use std::io::ErrorKind;
use openssl::ssl::{SslMethod, SslConnector, SslVerifyMode, SslStream};
use std::net::{TcpStream, Shutdown};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::collections::HashMap;

fn send(s: &mut SslStream<TcpStream>, msg: &String) {
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

fn ident(s: &mut SslStream<TcpStream>, nick: &String) {
    send(s, &format!("NICK {}", nick));
    send(s, &format!("USER {} 0 * :{}", nick, nick));
}

fn join(s: &mut SslStream<TcpStream>, channel: &String) {
    send(s, &format!("JOIN :{}", channel))
}

fn say(s: &mut SslStream<TcpStream>, target: &String, what: &String) {
    send(s, &format!("PRIVMSG {} :{}", target, what));
}

fn quit(s: &mut SslStream<TcpStream>, msg: &String) {
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


fn on_welcome(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, msg: &IrcMessage) {
    join(stream, &bot.channel);
    say(stream, &bot.channel, &String::from("high"));
}

fn on_privmsg(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, msg: &IrcMessage) {
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
            rest = String::from(&msg.args[1][end_idx+1..]);
        } else {
            rest = String::from("");
        }
        println!("command: {} / rest: {}", command, rest);
        bot.dispatch(stream, &msg, &command, &rest);
    }
}

fn on_ping(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, msg: &IrcMessage) {
    let mut out = String::from("PONG :");
    out.push_str(&msg.args[0]);
    send(stream, &out);
}

type CallbackHandler = fn(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, message: &IrcMessage);
type Command = fn(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, message: &IrcMessage, rest: &String);


struct IrcBot {
    host: String,
    nick: String,
    channel: String,
}

impl IrcBot {
    fn new(host: String, nick: String, channel: String) -> IrcBot {
        IrcBot {
            host: host,
            nick: nick,
            channel: channel,
        }
    }

    fn dispatch(&mut self, stream: &mut SslStream<TcpStream>, msg: &IrcMessage, command: &String, rest: &String) {
        let handler: Option<Command> = match command.as_str() {
            "weather" => Some(command_weather),
            _ => None,
        };
        if handler.is_some() {
            handler.unwrap()(self, stream, &msg, rest);
        }
    }
}

fn connect(bot: &IrcBot) -> SslStream<TcpStream> {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = builder.build();
    let tcp_stream = TcpStream::connect(&bot.host).unwrap();
    tcp_stream.set_nodelay(true);
    tcp_stream.set_read_timeout(Some(Duration::new(1, 0)));
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

fn command_weather(bot: &mut IrcBot, stream: &mut SslStream<TcpStream>, message: &IrcMessage, rest: &String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 1 {
        return;
    }

    let key = env::var("OPENWEATHER_API_KEY").unwrap();
    let url = format!("https://api.openweathermap.org/data/2.5/weather?q={},us&APPID={}", parts[0], key);
    println!("weather: {}", url);
    let body = reqwest::blocking::get(&url).unwrap().json::<WeatherResponse>();
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

fn main() {

    let args: Vec<String> = env::args().collect();
    let mut bot: IrcBot = IrcBot::new(String::from(&args[1]), String::from(&args[2]), String::from(&args[3]));

    let mut stream = connect(&bot);
    static running: AtomicBool = AtomicBool::new(true);
    ctrlc::set_handler(|| {
        println!("shutdown..");
        running.store(false, Ordering::Relaxed);
    });

    ident(&mut stream, &bot.nick);

    while running.load(Ordering::Relaxed) {
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
                    },
                }
            }
        }
    }

    quit(&mut stream, &String::from("out"));
    stream.shutdown();
}
