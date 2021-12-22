use crate::{IrcMessage, IrcConnection, IrcBot, Result, say};

pub fn command(bot: &mut IrcBot, stream: &mut IrcConnection, message: &IrcMessage, rest: &String) -> Result<()> {
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
