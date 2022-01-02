extern crate chrono;
extern crate clap;
extern crate ctrlc;
extern crate lazy_static;
extern crate linkify;
extern crate regex;
extern crate rustls;

use std::collections::HashMap;
use std::convert::TryInto;
use std::error;
use std::fmt;
use std::io::prelude::*;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration as Timeout;

use chrono::{DateTime, Duration, Utc};
use clap::{App, Arg};
use env_logger;
use linkify::{LinkFinder, LinkKind};
use log;
use rand::seq::IteratorRandom;
use regex::Regex;
use rusqlite::{params, Connection, Result as SQLResult, OptionalExtension};
use rustls::client::HandshakeSignatureValid;
use rustls::{ClientConfig, ClientConnection, Stream};

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;
type IrcConnection<'a> = Stream<'a, ClientConnection, TcpStream>;

mod commands;
mod utils;

#[derive(Debug, Clone)]
struct Error {
    msg: String,
}

impl Error {
    fn new(msg: &str) -> Error {
        Error {
            msg: msg.to_string(),
        }
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

fn send(s: &mut IrcConnection, msg: &String) -> Result<()> {
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

#[derive(Debug, Clone)]
pub struct IrcPrefix {
    host: String,
    nick: String,
    realname: String,
}

fn parse_prefix(s: &String) -> IrcPrefix {
    let (host, nick, realname);

    if let Some(host_idx) = s.find("@") {
        host = String::from(&s[host_idx + 1..]);

        let prefix = String::from(&s[..host_idx]);
        if let Some(prefix_idx) = prefix.find("!") {
            nick = String::from(&prefix[..prefix_idx]);
            realname = String::from(&prefix[prefix_idx + 1..]);
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
        host,
        nick,
        realname,
    }
}

#[derive(Debug, Clone)]
pub struct IrcMessage {
    prefix: IrcPrefix,
    command: String,
    args: Vec<String>,
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
        let trailing = String::from(&s[trailing_idx + 2..]);
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
        args: args,
    }
}

fn on_welcome(bot: &mut IrcBot, stream: &mut IrcConnection, _msg: &IrcMessage) -> Result<()> {
    join(stream, &bot.channel)?;
    say(stream, &bot.channel, &random_greeting())?;
    Ok(())
}

fn on_command(bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
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
        rest = String::from(msg.args[1][end_idx + 1..].trim());
        } else {
        rest = String::from("");
    }
    bot.dispatch(stream, &msg, &command, &rest)?;
    Ok(())
}

fn on_privmsg(bot: &mut IrcBot, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
    if msg.args[0] != bot.channel {
        return Ok(());
    }

    let ident = bot.ensure_ident(msg)?;

    let mut prefix = String::from(&bot.nick);
    prefix.push_str(": ");
    if msg.args[1].starts_with(&prefix) {
        say(stream, &bot.channel, &random_greeting())?;
    } else if msg.args[1].starts_with("!") {
        on_command(bot, stream, msg)?;
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

type CallbackHandler =
    fn(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage) -> Result<()>;

type Command = fn(
    bot: &mut IrcBot,
    stream: &mut IrcConnection,
    message: &IrcMessage,
    rest: &String,
) -> Result<()>;

#[derive(Debug)]
pub struct SeenUrl {
    owner: String,
    count: i32,
    first: DateTime<Utc>,
}

#[derive(Debug)]
pub struct Ident {
    id: i64,
    host: String,
    nick: String,
    realname: String,
}

#[derive(Debug)]
pub struct IrcBot {
    host: String,
    nick: String,
    channel: String,
    ignore: Option<Vec<String>>,

    db: Connection,

    seen_urls: HashMap<String, SeenUrl>,
    last_greet: DateTime<Utc>,
}

static CREATE_TABLE_SEEN_IDENTS: &str = "
CREATE TABLE IF NOT EXISTS seen_idents (
    id INTEGER PRIMARY KEY,
    nick TEXT,
    realname TEXT,
    host TEXT,
    last_seen DATETIME NOT NULL
);
";

static CREATE_TABLE_SEEN_URLS: &str = "
CREATE TABLE IF NOT EXISTS seen_urls (
    id INTEGER PRIMARY KEY,
    owner_id INTEGER NOT NULL,
    url_hash TEXT NOT NULL UNIQUE,
    count INTEGER NOT NULL DEFAULT 1,
    seen DATETIME NOT NULL
);
";


static GREETINGS: &[&str] = &[
    "hi", "high", "hello", "sirs", "pals", "buddies", "friends", "amigos", "compadres", "mates", "chums", "confidants", "brothers"
];

fn random_greeting() -> String {
    let mut rng = rand::thread_rng();
    return String::from(*GREETINGS.iter().choose(&mut rng).unwrap());
}


impl IrcBot {
    fn new(host: String, nick: String, channel: String, db: Connection) -> IrcBot {
        return IrcBot {
            host: host,
            nick: nick,
            channel: channel,
            ignore: None,
            db: db,
            seen_urls: HashMap::new(),
            last_greet: Utc::now(),
        };
    }

    fn init(&mut self) -> Result<()> {
        self.db.execute(CREATE_TABLE_SEEN_IDENTS, [])?;
        self.db.execute(CREATE_TABLE_SEEN_URLS, [])?;
        commands::nega::init(self)?;
        Ok(())
    }

    fn set_ignore(&mut self, ignore: Option<Vec<String>>) {
        self.ignore = ignore;
    }

    // FIXME move to commands/mod.rs
    fn dispatch(
        &mut self,
        stream: &mut IrcConnection,
        msg: &IrcMessage,
        command: &String,
        rest: &String,
    ) -> Result<()> {
        let handler: Option<Command> = match command.as_str() {
            "weather" => Some(commands::weather::command),
            "ud" => Some(commands::ud::command),
            //"giphy" => Some(commands::giphy::command),
            "strain" => Some(commands::strain::command),
            "nega" => Some(commands::nega::command_nega),
            "kudos" => Some(commands::nega::command_kudos),
            //"image" => Some(commands::image::command),
            _ => None,
        };

        if let Some(handler_fn) = handler {
            if let Err(e) = handler_fn(self, stream, &msg, rest) {
                log::error!("command errored: {}", e);
            }
        }

        Ok(())
    }

    fn get_ident(&mut self, msg: &IrcMessage) -> Option<Ident> {
        return self.db.query_row(
            "SELECT id, host, nick, realname FROM seen_idents WHERE host=?1 AND nick=?2 AND realname=?3",
            params![msg.prefix.host, msg.prefix.nick, msg.prefix.realname],
            |row| {
               Ok(Ident {
                    id: row.get(0)?,
                    host: row.get(1)?,
                    nick: row.get(2)?,
                    realname: row.get(3)?,
                })
            }).optional().unwrap()
    }

    fn update_last_seen(&mut self, ident: &Ident) -> Result<()> {
        self.db.execute("UPDATE seen_idents SET last_seen=?1 WHERE id=?2", params![Utc::now(), ident.id])?;
        Ok(())
    }

    fn add_ident(&mut self, msg: &IrcMessage) -> Result<Ident> {
        self.db.execute(
            "INSERT INTO seen_idents(host, nick, realname, last_seen) VALUES (?1, ?2, ?3, ?4)",
            params![msg.prefix.host, msg.prefix.nick, msg.prefix.realname, Utc::now()],
        )?;

        Ok(Ident {
            id: self.db.last_insert_rowid(),
            host: msg.prefix.host.clone(),
            nick: msg.prefix.nick.clone(),
            realname: msg.prefix.realname.clone(),
        })
    }

    fn ensure_ident(&mut self, msg: &IrcMessage) -> Result<Ident> {
        if let Some(ident) = self.get_ident(msg) {
            self.update_last_seen(&ident)?;
            return Ok(ident);
        }
        return self.add_ident(msg);
    }

    fn find_ident_by_nick(&mut self, nick: &String) -> Option<Ident> {
        return self.db.query_row(
            "SELECT id, host, nick, realname FROM seen_idents WHERE nick=?1 ORDER BY last_seen DESC",
            params![nick],
            |row| {
               Ok(Ident {
                    id: row.get(0)?,
                    host: row.get(1)?,
                    nick: row.get(2)?,
                    realname: row.get(3)?,
                })
            }).optional().unwrap()
    }

    fn see(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
        self.scrape_urls(stream, msg)?;
        self.check_greeting(stream, msg)?;
        self.check_emote(stream, msg)?;
        Ok(())
    }

    fn check_greeting(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {

        let now = Utc::now();
        let should_greet = (self.last_greet + Duration::minutes(5)) < now;
        if msg.args.len() == 2 && GREETINGS.contains(&msg.args[1].as_str()) && should_greet {
            let target = &msg.args[0];
            say(
                stream,
                &target,
                &random_greeting(),
            )?;
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

    fn handle_url(&mut self, stream: &mut IrcConnection, msg: &IrcMessage, url: &str) -> Result<()> {
        match self.seen_urls.get_mut(url) {
            Some(seen) => {
                if msg.prefix.nick != seen.owner {
                    seen.count += 1;
                    let target = &msg.args[0];
                    say(
                        stream,
                        &target,
                        &format!(
                            "repost: {} (first seen at {} by {} / repost count: {})",
                            url,
                            seen.first.to_rfc2822(),
                            seen.owner,
                            seen.count
                        ),
                    )?;
                }
            }
            None => {
                self.seen_urls.insert(
                    url.to_string(),
                    SeenUrl {
                        owner: String::from(&msg.prefix.nick),
                        count: 1,
                        first: Utc::now(),
                    },
                );
            }
        }

        Ok(())
    }

    fn scrape_urls(&mut self, stream: &mut IrcConnection, msg: &IrcMessage) -> Result<()> {
        let mut finder = LinkFinder::new();
        finder.url_must_have_scheme(false);
        finder.kinds(&[LinkKind::Url]);
        let text = msg.args[1..].join(" ");
        let links: Vec<_> = finder.links(&text).collect();
        for link in links.iter() {
            let url = link.as_str();
            self.handle_url(stream, msg, url)?;
        }
        Ok(())
    }
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

    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }
        let mut buffer = [0; 4096];
        match stream.read(&mut buffer) {
                Ok(bytes) => {
                    let s = String::from_utf8_lossy(&buffer[0..bytes]).to_string();
                    if let Err(e) = handle_message(s, bot, stream) {
                        log::error!("error handling message: {}", e);
                    }
                }
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

struct NoCertificateVerification {}

impl rustls::client::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::Certificate,
        dss: &rustls::internal::msgs::handshake::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
        return Ok(HandshakeSignatureValid::assertion());
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::Certificate,
        dss: &rustls::internal::msgs::handshake::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        return Ok(HandshakeSignatureValid::assertion());
    }
}


fn main() -> Result<()> {
    env_logger::init();

    let args = App::new("rusty")
        .version("0.1")
        .arg(Arg::new("host").takes_value(true).required(true).index(1))
        .arg(Arg::new("nick").takes_value(true).required(true).index(2))
        .arg(
            Arg::new("channel")
                .takes_value(true)
                .required(true)
                .index(3),
        )
        .arg(
            Arg::new("ignore")
                .takes_value(true)
                .long("ignore")
                .multiple_occurrences(true),
        )
        .get_matches();

    let host = String::from(args.value_of("host").unwrap());
    let nick = String::from(args.value_of("nick").unwrap());
    let channel = String::from(args.value_of("channel").unwrap());

    let db_path = format!("./{nick}-at-{host}.db", host=host, nick=nick);
    let db = Connection::open(&db_path).expect("cannot open db");

    let mut bot: IrcBot = IrcBot::new(host, nick, channel, db);
    bot.init()?;

    if let Some(ignore) = args.values_of("ignore") {
        let values: Vec<String> = ignore.map(|s| s.to_string()).collect();
        bot.set_ignore(Some(values));
    }

    let verifier = Arc::new(NoCertificateVerification {});
    let config = ClientConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_safe_default_protocol_versions()?
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    static RUNNING: AtomicBool = AtomicBool::new(true);
    ctrlc::set_handler(|| {
        log::info!("shutdown..");
        RUNNING.store(false, Ordering::Relaxed);
    })?;

    while RUNNING.load(Ordering::Relaxed) {
        log::info!("connecting to {}", bot.host);

        let mut host_parts = bot.host.split(":");
        let server_name;
        if let Some(host) = host_parts.next() {
            server_name = host.try_into()?;
        } else {
            server_name = "".try_into()?;
        }

        let mut conn = ClientConnection::new(Arc::new(config.clone()), server_name)?;
        let mut tcp_stream = TcpStream::connect(&bot.host)?;
        tcp_stream.set_read_timeout(Some(Timeout::from_millis(1000)))?;
        let mut stream = Stream::new(&mut conn, &mut tcp_stream);
        if let Err(e) = bot_main(&RUNNING, &mut bot, &mut stream) {
            log::error!("bot errored: {}", e);
        }

        stream.conn.send_close_notify();
        stream.sock.shutdown(std::net::Shutdown::Both)?;

        if RUNNING.load(Ordering::Relaxed) {
            thread::sleep(Timeout::from_millis(5000));
        }
    }

    Ok(())
}
