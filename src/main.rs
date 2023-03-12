use std::{
    env,
    time::Duration,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use rand::Rng;
//use tokio::io;
use std::process::Stdio;
use tokio::io::{BufReader, AsyncBufReadExt};

use std::convert::TryInto;

use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    framework::{
        standard::{
            macros::{command, group},
            Args,
            CommandResult,
        },
        StandardFramework,
    },
    http::Http,
    model::{channel::Message, Timestamp, gateway::Ready, prelude::ChannelId},
    prelude::{GatewayIntents, Mentionable},
    Result as SerenityResult,
};

use songbird::{
    input::restartable::Restartable,
    Event,
    EventContext,
    EventHandler as VoiceEventHandler,
    SerenityInit,
    TrackEvent, tracks::{TrackHandle, PlayMode},
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}


#[group]
#[commands(
    deafen, join, leave, play, skip, stop, pause, resume, undeafen, queue, remove, seek, playlist
)]
struct General;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let framework = if cfg!(debug_assertions){
        println!("debug");
        StandardFramework::new()
        .configure(|c| c.prefix("~"))
        .group(&GENERAL_GROUP)
    } else {
        println!("release");
        StandardFramework::new()
        .configure(|c| c.prefix("!"))
        .group(&GENERAL_GROUP)
    };
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

     

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Err creating client");

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}

#[command]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        },
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(msg.channel_id.say(&ctx.http, "Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[command]
#[aliases(tits)]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        },
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let (_handle_lock, success) = manager.join(guild_id, connect_to).await;

    if let Ok(_channel) = success {} else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Error joining the channel")
                .await,
        );
    }

    Ok(())
}

#[command]
#[aliases(gtfo)]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        //check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

#[command]
#[aliases(p)]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    join(ctx, msg, args.clone() ).await.unwrap();
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, "Must provide a URL to a video or audio")
                    .await,
            );

            return Ok(());
        },
    };

    if !url.starts_with("http") {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Must provide a valid URL")
                .await,
        );

        return Ok(());
    }

    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        // Here, we use lazy restartable sources to make sure that we don't pay
        // for decoding, playback on tracks which aren't actually live yet.
        let source = match Restartable::ytdl(url, true).await {
            Ok(source) => source,
            Err(why) => {
                println!("Err starting source: {:?}", why);

                //check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);
                check_msg(msg.channel_id.say(&ctx.http, format!("Err starting source: {:?}", why)).await);

                return Ok(());
            },
        };

        handler.enqueue_source(source.into());
        match args.single::<String>() {
            Ok(now) => {
                if now == "now" {
                    let front = handler.queue().modify_queue(|a| a.pop_back().unwrap());
                    handler.queue().modify_queue(|a| a.insert(1, front)) ;
                let title =  handler.queue().current_queue().get(1).unwrap().metadata().title.clone().unwrap();
                check_msg(
                    msg.channel_id
                        .say(
                            &ctx.http,
                            //format!("Added song to queue: position {}", handler.queue().len()),
                            format!("Added song to the front of the queue: **{}** ", title),
                        )
                        .await,
                );
                    return Ok(());
                }
            },
            Err(_) => {
            let title =  handler.queue().current_queue().last().unwrap().metadata().title.clone().unwrap();
            check_msg(
                msg.channel_id
                    .say(
                        &ctx.http,
                        //format!("Added song to queue: position {}", handler.queue().len()),
                        format!("Added song to queue: **{}** at position **{}**", title , handler.queue().len()),
                    )
                    .await,
            );
                return Ok(());
            },
        };
        

    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[aliases(pl)]
#[only_in(guilds)]
async fn playlist(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    join(ctx, msg, args.clone() ).await.unwrap();
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, "Must provide a URL to a video or audio")
                    .await,
            );

            return Ok(());
        },
    };

    if !url.starts_with("http") & !url.contains("list") {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Must provide a valid Youtube Playlist URL")
                .await,
        );

        return Ok(());
    }

    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        // Here, we use lazy restartable sources to make sure that we don't pay
        // for decoding, playback on tracks which aren't actually live yet.
        let mut songs_in_playlist = Vec::default();
            std::str::from_utf8(
                &tokio::process::Command::new("yt-dlp")
                    .args(["yt-dlp", "--flat-playlist", "--get-id","--compat-options", "no-youtube-unavailable-videos", &url])
                    .output()
                    .await
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .to_string()
            .split('\n')
            .filter(|f| !f.is_empty())
            .for_each(|id| {
                let song = format!("https://www.youtube.com/watch?v={id}");

                songs_in_playlist.push(song);
            });
        let slice = args.single::<usize>().unwrap_or(1);
        songs_in_playlist = songs_in_playlist.drain(slice-1 ..).collect();
        songs_in_playlist.truncate(50);
        for song in songs_in_playlist{
        let source = match Restartable::ytdl(song, true).await {
            Ok(source) => source,
            Err(why) => {
                println!("Err starting source: {:?}", why);
                check_msg(msg.channel_id.say(&ctx.http, format!("Err starting source: {:?}", why)).await);
                continue;
            },
        };

        handler.enqueue_source(source.into());

        }


        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    //format!("Added song to queue: position {}", handler.queue().len()),
                    format!("**{}** Songs in queue", handler.queue().len()),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}


#[command]
#[aliases(s)]
#[only_in(guilds)]
async fn skip(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.skip();


        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song skipped: {} in queue.", queue.len()),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}
#[command]
#[aliases(r)]
#[only_in(guilds)]
async fn remove(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let no = args.single::<usize>().unwrap();
        let _ = handler.queue().dequeue(no).unwrap();
        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song removed"),
                )
                .await,
            );
        } else { 
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn resume(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.resume();

        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song resumed"),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}


#[command]
#[only_in(guilds)]
async fn pause(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.pause();

        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song paused"),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn seek(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let no = args.current().unwrap().parse::<u64>()?;
        let time = Duration::new(no, 0);
        let _ = queue.current().unwrap().seek_time(time).unwrap();

        check_msg(
            msg.channel_id
                .say(
                    &ctx.http,
                    format!("Song seeked to: {}:{}", no/60, no%60),
                )
                .await,
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}


#[command]
#[aliases(cl, clear)]
#[only_in(guilds)]
async fn stop(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue();
        let _ = queue.stop();

        check_msg(msg.channel_id.say(&ctx.http, "Queue cleared.").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to undeafen in")
                .await,
        );
    }

    Ok(())
}


#[command]
#[aliases(q)]
#[only_in(guilds)]
async fn queue(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(&ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let handler = handler_lock.lock().await;
        let queue = handler.queue().current_queue();
        
        
                if !queue.is_empty() {
            let mut queue_str = String::new();
            let metadata = queue[0].metadata();
            let info = queue[0].get_info().await.unwrap();
            queue_str += &format!(
                "__**Now playing:**__\n```yaml\n{} | {}:{:02}/{}:{:02}\n```",
                &metadata.title.clone().unwrap(),
                info.position.as_secs() / 60,
                info.position.as_secs() % 60,
                metadata.duration.unwrap().as_secs()/60,
                metadata.duration.unwrap().as_secs()%60
            );
            if queue.len() > 1 {
                let page = args.single::<usize>().unwrap_or(1);
                queue_str += "\n__**Queue:**__\n```yaml\n";
                for (index, track) in queue[1+(page-1)*10..].iter().take(10).enumerate() {
                    let metadata = track.metadata();
                    queue_str += &format!(
                        "{}: {} | {}:{:02}\n",
                        index + 1+10*(page-1),
                        &metadata.title.clone().unwrap(),
                        metadata.duration.unwrap().as_secs()/60,
                        metadata.duration.unwrap().as_secs()%60
                    );
                }
                if queue.len() > 10 {
                    queue_str += &format!("page {}/{}", page, (queue.len()+9)/10);
                }
                queue_str += "\n```";
            }
            queue_str = queue_str.replace("@", "@\u{200B}");
            msg.channel_id
                .send_message(ctx.clone(), |m| {
                    m.embed(|e| {
                        e.description(&queue_str);
                        e.image(metadata.thumbnail.clone().unwrap().as_str());
                        e
                    })
                })
                .await?;
        } else {
            msg.channel_id.say(ctx, "Q is empty").await?;
        }    
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }
    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}

