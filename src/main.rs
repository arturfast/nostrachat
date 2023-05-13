use std::io::Read;
use std::fs;
use std::fs::File;
use std::process::exit;
use std::env::temp_dir;
use std::collections::HashMap;

use rustyline::error;
use rustyline::validate::{ ValidationResult::Valid, ValidationResult::Invalid, ValidationContext, ValidationResult, Validator};
use rustyline::{ Editor, Completer, Helper, Highlighter, Hinter };
use rustyline::history::FileHistory;

use colored::Colorize;
use serde::{ Deserialize, Serialize };
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};
use nostr::prelude::*;
use nostr::prelude::secp256k1::PublicKey;

use futures_util::{StreamExt, SinkExt};
use futures::stream::SplitStream;
use futures::stream::SplitSink;

use chats::{ Chat, ChatType, PrivateChat, PublicChannel };
use crypto::{ RatchetProfile };

mod ascii_art;
mod ui;
mod crypto;
mod chats;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    theme: ThemeConfig,
    relays: Vec<String>,
    channels: Vec<String>,
    chats: Vec<String>,
    privkey: String,
    pubkey: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ThemeConfig {
    shadow: bool,
    borders: String,
    colors: ThemeColors
}

#[derive(Debug,Clone, Deserialize, Serialize)]
struct ThemeColors {
    background: String,
    view: String,
    primary: String,
    secondary: String,
    tertiary: String,
    title_primary: String,
    highlight: String,
    highlight_inactive: String
}

impl Config {
    fn new() -> Config {
         let content: String = fs::read_to_string("config.toml").unwrap();
         let config_contents: Config = toml::from_str(&content).unwrap();
         return config_contents;
    }
}

#[derive(Completer, Helper, Highlighter, Hinter)]
struct InputValidator {}

impl Validator for InputValidator {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> { 
        let input = ctx.input();
        let result = if input.is_empty() {
            Invalid(Some("".to_string()))
        } else {
            Valid(None)
        };
        Ok(result) 
    }
}

#[tokio::main]
async fn main() {

    let config: Config = Config::new();
    let key_pair = Keys::new(SecretKey::from_bech32(&config.privkey).unwrap());
    let relay = ui::select_relay(config.clone());
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
    println!("Public key bech32: {}", key_pair.public_key().to_bech32().unwrap());
    println!("Connecting to {}", relay.green());

    let (socket, _response) = connect_async(&relay).await.expect("Failed to connect");
    let (mut writer, mut reader) = socket.split();

    let mut rl = Editor::new().unwrap();

    let channel_list: Vec<PublicChannel> = match get_channel_list(&mut writer, &mut reader, Some(config.channels.clone())).await {
        Ok(val) => val,
        Err(why) => panic!("{}", why),
    }; 
    
    let private_chats: Vec<PrivateChat> = config.chats.iter().map(|contact_pubkey| PrivateChat {
        name: contact_pubkey.to_string(), // TODO: Fetch name from server somehow, like with get_channel_list
        recipient_public_key: XOnlyPublicKey::from_bech32(contact_pubkey).unwrap(),
        secret_key: key_pair.secret_key().unwrap(),
        ratchet_profile: RatchetProfile::new(key_pair.secret_key().unwrap(), XOnlyPublicKey::from_bech32(contact_pubkey).unwrap().public_key(Parity::Even)),
    }).collect();
    
    // Clears terminal and sets cursor to the start
    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);

    let mut chat = match ui::select_chat(config.clone(), channel_list.clone(), private_chats.clone()) {
        Some(val) => {
            val
        }, 
        None => {
            ChatType::PublicChannel(ui::select_unknown_channel(config.clone(), get_channel_list(&mut writer, &mut reader, None).await.unwrap()))
        }
    };

    //print_channel_info(&relay, &channel); TODO: Print channel/chat info.

    let pubkeys_to_colors: HashMap<String, u8> = HashMap::new();
    let printing_handler = {
        chats::PrintingHandler {
            printer: rl.create_external_printer().unwrap(),
            pubkeys_to_colors: pubkeys_to_colors,
            public_key: key_pair.public_key(),
        }
    };

    writer.send(chat.build_request_message()).await.expect("Couldn't write message to websocket!");
    let ws_to_stdout = chat.clone().print_incoming_events(printing_handler, reader);

    tokio::spawn(ws_to_stdout);
    
    loop {
        let input = prompt(key_pair.public_key().to_bech32().unwrap()[4 .. 10].to_string(), &mut rl);

        match input.as_str() {
            "/help" => {
                let help_text = "/help		- Prints this help message\n/editor		- Opens a text editor to type your message out\n/channelinfo       - Shows metadata about the current channel\n/exit		- Quits Nostrachat\n";
                println!("{}", help_text.truecolor(128, 128, 128));
            },
            "/exit" => {
                println!("Goodbye!");
                exit(0);
            },
            "/editor" => {
                let msg = chat.message_from(editor().expect("Couldn't open editor!"), key_pair.secret_key().unwrap());
                writer.send(msg).await.expect("Couldn't sent message over websocket!");
//              writer.send(channel_event(editor().expect("Couldn't open the editor."), channel)).await.expect("Impossible to send message");
            },
            "/channelinfo" => {
                println!("{}", chat.get_info_table(&relay));
            },
            &_ => {
                if &input[0 .. 1] == "/" {
                    eprintln!("Command not found! Get all commands with /help");
                    continue;
                }
                //writer.send(private_event(input, KeyPair::from_secret_key(&chat.secret_key), chat.recipient_public_key)).await.expect("Impossible to send message");
                let msg = chat.message_from(input, key_pair.secret_key().unwrap());
                writer.send(msg).await.expect("Couldn't sent message over websocket!");
            }
        }
    }
}

fn prompt(name: String, rl: &mut Editor<InputValidator, FileHistory>) -> String {
    if rl.load_history("history.txt").is_err() {
        println!("No previous history.");
    } 
    let validator_for_empty_input = InputValidator { };
    rl.set_helper(Some(validator_for_empty_input));
    let readline = rl.readline(&format!("[{}] ", name.green()));
      return match readline {
        Ok(line) => { 
            rl.add_history_entry(line.as_str()).unwrap();
                rl.save_history("history.txt").unwrap();
                line
            }

        Err(err) => match err {
            error::ReadlineError::Interrupted => {
                println!("Goodbye!");
                exit(2);
            },
            error::ReadlineError::Eof => {
                println!("Goodbye!");
                exit(0);
            },
            _ => {
                panic!("Error {:?}", err);
            }
        }
    };
}

fn editor() -> Result<String> {
   let mut temporary_file = temp_dir();
   temporary_file.push("nostrachat-buffer.txt");
   fs::write(&temporary_file, b"*Type out your message here*")?;

   open::that(&temporary_file)?;
   let mut file = File::open(temporary_file.as_path())?;
   let mut content = String::new();
   file.read_to_string(&mut content)?;
   return Ok(content);
}

async fn get_channel_list(writer: &mut SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, tokio_tungstenite::tungstenite::Message>, reader: &mut SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>, ids: Option<Vec<String>>) -> Result<Vec<PublicChannel>> {
   let mut list: Vec<PublicChannel> = Vec::new();
   let mut filter = Filter::default();
   filter.kinds = Some(vec![Kind::Custom(40)]);
   filter.ids = match ids {
        Some(val) => Some(val),
        None => None,
   };
   let req = ClientMessage::new_req(SubscriptionId::generate(), vec![filter]).as_json();
   writer.send(Message::Text(req.clone())).await.expect("Error");

    loop {
        let event_text = reader.next().await.unwrap().unwrap().to_string();

        let json_val: Value = match serde_json::from_str(&event_text) {
            Ok(val) => val,
            Err(why) => {
                eprintln!("Faulty JSON: {}", why);
                continue;
            }
        };

        match json_val[0].as_str().unwrap() {
            "EOSE" => {
                break;
            },
            "NOTICE" => {
                println!("NOTICE: {:?}", &json_val);
                break;
            }
            &_ => { }
        }

        let event = Event::from_json(&json_val[2].to_string()).unwrap();
        let metadata = match Metadata::from_json(json_val[2]["content"].as_str().unwrap()) {
            Ok(val) => val, 
            Err(error) => {
                eprintln!("Poorly formatted event. {}", error);
                continue;
            }
        };
        
        list.push(PublicChannel { root_event: event, metadata: metadata });
   }
   return Ok(list);
}
