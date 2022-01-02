use chrono::Utc;
use rusqlite::params;

use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

static CREATE_TABLE_NEGA_VOTES: &str = "
CREATE TABLE IF NOT EXISTS nega_votes (
    id INTEGER PRIMARY KEY,
    created DATETIME NOT NULL,
    submitted_by INTEGER NOT NULL,
    subject INTEGER NOT NULL,
    vote INTEGER NOT NULL,
    reason TEXT NOT NULL
);
";


pub fn init(bot: &mut IrcBot) -> Result<()> {
    bot.db.execute(CREATE_TABLE_NEGA_VOTES, [])?;
    Ok(())
}


pub fn record_vote(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String, vote: i8) -> Result<()> {

    if rest.len() < 1 {
        return Ok(());
    }

    let ident = bot.get_ident(message).unwrap();

    let mut split_iter = rest.splitn(2, " ");
    let target_nick_opt = split_iter.next();
    if target_nick_opt.is_none() {
        return Ok(());
    }

    let target_nick = target_nick_opt.unwrap();

    let target_opt = bot.find_ident_by_nick(&target_nick.to_string());
    if target_opt.is_none() {
        return Ok(());
    }

    let target = target_opt.unwrap();

    let reason_opt = split_iter.next();
    if reason_opt.is_none() {
        return Ok(());
    }
    let reason = reason_opt.unwrap();

    bot.db.execute(
        "INSERT INTO nega_votes(created, submitted_by, subject, vote, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![Utc::now(), ident.id, target.id, vote, reason]
    )?;


    Ok(())
}

pub fn command_nega(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    return record_vote(bot, stream, message, rest, -1);
}


pub fn command_kudos(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
    return record_vote(bot, stream, message, rest, 1);
}
