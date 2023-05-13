use std::sync::mpsc::{self};

use cursive::theme::{ Effect, Style, PaletteColor, load_toml};
use cursive::views::{ Button, OnEventView, SelectView, TextView, Dialog, LinearLayout, TextContent };
use cursive::align::HAlign;
use cursive::utils::span::SpannedString;
use cursive::event::EventResult;
use cursive::traits::Scrollable;
use cursive::{ Cursive, CursiveRunnable };

use crate::Config;
use crate::ascii_art;
use crate::chats::{ ChatType, Chat, PrivateChat, PublicChannel };

pub fn select_relay(config: Config) -> String {
    let mut siv: CursiveRunnable = get_configured_siv(&config);
    let mut relay_view: OnEventView<SelectView<String>> = setup_chat(config.relays.clone(), config.relays.clone());
    let (tx, rx) = mpsc::channel();

    relay_view.get_inner_mut().set_on_submit(move |s: &mut Cursive, item: &String| {
        tx.send(item.clone()).unwrap();
        s.quit();
    });

    let linear_layout: LinearLayout = LinearLayout::vertical()
        .child(relay_view.scrollable());

    siv.add_layer(
        linear_layout
    );

    siv.run();

    rx.recv().unwrap().to_string()
}

pub fn select_chat(config: Config, channel_list: Vec<PublicChannel>, private_chats: Vec<PrivateChat>) -> Option<ChatType> {

    let mut siv: CursiveRunnable = get_configured_siv(&config);

    let public_chat_names: Vec<String> = channel_list.iter().map(|channel| channel.clone().get_name()).collect();
    let private_chat_names: Vec<String> = private_chats.iter().map(|chat| chat.clone().get_name()).collect();

    let mut select_public_chat = setup_chat(public_chat_names.clone(), channel_list.clone());
    let mut select_private_chat = setup_chat(private_chat_names.clone(), private_chats.clone());
   
    let (tx, rx) = crossbeam_channel::bounded(1);
    // TODO: Find a better way to access the same channel receiver, without tx_clone variables.
    let tx_clone = tx.clone();
    let tx_clone2 = tx.clone();

    select_public_chat.get_inner_mut().set_on_submit(move |s: &mut Cursive, item: &PublicChannel| {
        tx.send(Some(ChatType::PublicChannel(item.clone()))).expect("Couldn't submit selection.");
        s.quit();
    });

    select_private_chat.get_inner_mut().set_on_submit(move |s: &mut Cursive, item: &PrivateChat| {
        tx_clone.send(Some(ChatType::PrivateChat(item.clone()))).expect("Couldn't submit selection.");
        s.quit();
    });

    let term_width: usize = if let Some((w, _)) = term_size::dimensions() {
        w
    } else {
        eprintln!("Unable to get terminal width :(");
        0
    };
    
    let mut button_style = Style::default();
    button_style.effects = Effect::Bold.into();
    let button = Button::new_raw(SpannedString::styled("Search for more channels", button_style), move |s| { 
        tx_clone2.send(None).expect("Couldn't submit selection."); 
        s.quit();
    }); 

    let wall = "-";

    let wall_repetitions = if term_width >= 72 {
                               60 
                            } else {
                               if term_width >= 25 {
                                  term_width - 12
                               } else {
                                    panic!("Your terminal is too small! Set width to at least 25. Current width: {}", term_width);
                               }
                            };
    let top_wall = TextContent::new("┌".to_string() + &wall.repeat(wall_repetitions).to_string() + "┐");
    let bottom_wall = TextContent::new("└".to_string() + &wall.repeat(wall_repetitions).to_string() + "┘");
    let mut linear_layout: LinearLayout = LinearLayout::vertical()
        .child(TextView::new(std::str::from_utf8(&ascii_art::NOSTRACHAT_LOGO).unwrap()).h_align(HAlign::Center))
        .child(Dialog::around(TextView::new_with_content(top_wall.clone())).title("Private chats"))
        .child(select_private_chat.scrollable()) 
        .child(Dialog::around(TextView::new_with_content(bottom_wall.clone())))
        .child(Dialog::around(TextView::new_with_content(top_wall)).title("Public Channels"));
    if channel_list.is_empty() {
        linear_layout.add_child(TextView::new("No items found. Add them in your config.toml file.").h_align(HAlign::Center));
        linear_layout.add_child(select_public_chat.scrollable());
        linear_layout.add_child(button);
    } else {
        linear_layout.add_child(select_public_chat.scrollable());
        linear_layout.add_child(button);
    }
    linear_layout.add_child(Dialog::around(TextView::new_with_content(bottom_wall)));
        
    siv.add_layer(
        linear_layout
    );

    siv.run();

    return rx.recv().unwrap();
}

pub fn select_unknown_channel(config: Config, channels: Vec<PublicChannel>) -> PublicChannel {

    let channel_names: Vec<String> = channels.iter().map(|channel| channel.clone().get_name()).collect();

    let mut channel_view = setup_chat(channel_names.clone(), channels.clone());
    let mut siv: CursiveRunnable = get_configured_siv(&config);
    let (tx, rx) = mpsc::channel();

    channel_view.get_inner_mut().set_on_submit(move |s, item: &PublicChannel| {
        tx.send(item.to_owned()).unwrap();
        s.quit();
    });

    let mut bold_style = Style::default();
    bold_style.effects = Effect::Bold.into();
    let linear_layout: LinearLayout = LinearLayout::vertical()
        .child(Dialog::around(channel_view.scrollable()).title(SpannedString::styled("All channels on this relay", bold_style)));

    siv.add_layer(
        linear_layout
    );

    siv.run();
    return rx.recv().unwrap();
}

pub fn setup_chat<T: Clone + 'static>(label: Vec<String>, item: Vec<T>) -> OnEventView<SelectView<T>> {
    let mut chat_view: SelectView<T> = SelectView::new()
        .h_align(HAlign::Center)
        .autojump();
    chat_view.set_inactive_highlight(false);

    for i in 0..item.len() {
        chat_view.add_item(label[i].clone(), item[i].clone());
    }

    let chat_view_event = OnEventView::new(chat_view)
        .on_pre_event_inner('k', |s, _| {
            let cb = s.select_up(1);
            Some(EventResult::Consumed(Some(cb)))
        })
        .on_pre_event_inner('j', |s, _| {
            let cb = s.select_down(1);
            Some(EventResult::Consumed(Some(cb)))
        });
    
        return chat_view_event;
}

fn get_configured_siv(config: &Config) -> CursiveRunnable {
    let mut siv: CursiveRunnable = cursive::crossterm();
    let mut theme = match load_toml(&toml::to_string(&config.theme).unwrap()) {
       Ok(theme) => theme,
       Err(why) => { eprintln!("Config file is invalid.\n{:?}", why); panic!(); }
    };
    theme.palette[cursive::theme::PaletteStyle::Highlight] = cursive::theme::Style::from(theme.palette[PaletteColor::Highlight]).combine(cursive::theme::Effect::Bold);
    siv.set_theme(theme); 
    siv
}
