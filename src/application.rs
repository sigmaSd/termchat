use super::state::{State, CursorMovement, ChatMessage, MessageType, ScrollMovement};
use crate::terminal_events::{TerminalEventCollector};
use crate::renderer::{Renderer};
use crate::action::{Action, Processing};
use crate::commands::{CommandManager};
use crate::message::{NetMessage, Chunk};
use crate::util::{Error, Result, Reportable};
use crate::commands::send_file::{SendFileCommand};
use crate::commands::send_stream::{SendStreamCommand};

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};

use message_io::events::{EventQueue};
use message_io::network::{NetEvent, Network, Endpoint};

use std::net::{SocketAddrV4};
use std::io::{ErrorKind};
use std::collections::HashMap;
use minifb::Window;
use minifb::WindowOptions;

pub enum Event {
    Network(NetEvent<NetMessage>),
    Terminal(TermEvent),
    Action(Box<dyn Action>),
    // Close event with an optional error in case of failure
    // Close(None) means no error happened
    Close(Option<Error>),
}

pub struct Config {
    pub discovery_addr: SocketAddrV4,
    pub tcp_server_port: u16,
    pub user_name: String,
}

pub struct Application<'a> {
    config: &'a Config,
    state: State,
    network: Network,
    commands: CommandManager,
    //read_file_ev: ReadFile,
    _terminal_events: TerminalEventCollector,
    event_queue: EventQueue<Event>,
    windows: HashMap<Endpoint, Window>,
}

impl<'a> Application<'a> {
    pub fn new(config: &'a Config) -> Result<Application<'a>> {
        let mut event_queue = EventQueue::new();

        let sender = event_queue.sender().clone(); // Collect network events
        let network = Network::new(move |net_event| sender.send(Event::Network(net_event)));

        let sender = event_queue.sender().clone(); // Collect terminal events
        let _terminal_events = TerminalEventCollector::new(move |term_event| match term_event {
            Ok(event) => sender.send(Event::Terminal(event)),
            Err(e) => sender.send(Event::Close(Some(e))),
        })?;

        Ok(Application {
            config,
            state: State::default(),
            network,
            commands: CommandManager::default().with(SendFileCommand).with(SendStreamCommand),
            // Stored because we need its internal thread running until the Application was dropped
            _terminal_events,
            event_queue,
            windows: HashMap::new(),
        })
    }

    pub fn run(&mut self, out: impl std::io::Write) -> Result<()> {
        let mut renderer = Renderer::new(out)?;
        renderer.render(&self.state)?;

        let server_addr = ("0.0.0.0", self.config.tcp_server_port);
        let (_, server_addr) = self.network.listen_tcp(server_addr)?;
        self.network.listen_udp_multicast(self.config.discovery_addr)?;

        let discovery_endpoint = self.network.connect_udp(self.config.discovery_addr)?;
        let message = NetMessage::HelloLan(self.config.user_name.clone(), server_addr.port());
        self.network.send(discovery_endpoint, message);

        loop {
            match self.event_queue.receive() {
                Event::Network(net_event) => match net_event {
                    NetEvent::Message(endpoint, message) => {
                        self.process_network_message(endpoint, message);
                    }
                    NetEvent::AddedEndpoint(_) => (),
                    NetEvent::RemovedEndpoint(endpoint) => self.state.disconnected_user(endpoint),
                    NetEvent::DeserializationError(_) => (),
                },
                Event::Terminal(term_event) => {
                    self.process_terminal_event(term_event);
                }
                Event::Action(action) => {
                    self.process_action(action);
                }
                Event::Close(error) => {
                    return match error {
                        Some(error) => Err(error),
                        None => Ok(()),
                    }
                }
            }
            renderer.render(&self.state)?;
        }
        //Renderer is destroyed here and the terminal is recovered
    }

    fn process_network_message(&mut self, endpoint: Endpoint, message: NetMessage) {
        match message {
            // by udp (multicast):
            NetMessage::HelloLan(user, server_port) => {
                let server_addr = (endpoint.addr().ip(), server_port);
                if user != self.config.user_name {
                    let mut try_connect = || -> Result<()> {
                        let user_endpoint = self.network.connect_tcp(server_addr)?;
                        let message = NetMessage::HelloUser(self.config.user_name.clone());
                        self.network.send(user_endpoint, message);
                        self.state.connected_user(user_endpoint, &user);
                        Ok(())
                    };
                    try_connect().report_if_err(&mut self.state);
                }
            }
            // by tcp:
            NetMessage::HelloUser(user) => {
                self.state.connected_user(endpoint, &user);
            }
            NetMessage::UserMessage(content) => {
                if let Some(user) = self.state.user_name(endpoint) {
                    let message = ChatMessage::new(user.into(), MessageType::Text(content));
                    self.state.add_message(message);
                }
            }
            NetMessage::UserData(file_name, chunk) => {
                use std::io::Write;
                if self.state.user_name(endpoint).is_some() {
                    // safe unwrap due to check
                    let user = self.state.user_name(endpoint).unwrap().to_owned();

                    match chunk {
                        Chunk::Error => {
                            format!("'{}' had an error while sending '{}'", user, file_name)
                                .report_err(&mut self.state);
                        }
                        Chunk::End => {
                            format!(
                                "Successfully received file '{}' from user '{}'!",
                                file_name, user
                            )
                            .report_info(&mut self.state);
                        }
                        Chunk::Data(data) => {
                            let try_write = || -> Result<()> {
                                let user_path = std::env::temp_dir().join("termchat").join(&user);
                                match std::fs::create_dir_all(&user_path) {
                                    Ok(_) => (),
                                    Err(ref err) if err.kind() == ErrorKind::AlreadyExists => (),
                                    Err(e) => return Err(e.into()),
                                }

                                let file_path = user_path.join(file_name);
                                std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(file_path)?
                                    .write_all(&data)?;

                                Ok(())
                            };

                            try_write().report_if_err(&mut self.state);
                        }
                    }
                }
            }
            NetMessage::Stream(data) => {
                if let Some((data, width, height)) = data {
                    if !self.windows.contains_key(&endpoint) {
                        match Window::new("Stream", width, height, WindowOptions::default()) {
                            Ok(w) => {
                                self.windows.insert(endpoint, w);
                            }
                            Err(e) => {
                                e.to_string().report_err(&mut self.state);
                            }
                        }
                    }
                    assert_eq!(width / 2 * height, data.len());
                    if let Some(window) = self.windows.get_mut(&endpoint) {
                        window
                            .update_with_buffer(&data, width / 2, height)
                            .report_if_err(&mut self.state);
                    }
                } else {
                    if self.windows.contains_key(&endpoint) {
                        self.windows.remove(&endpoint);
                    }
                }
            }
        }
    }

    fn process_terminal_event(&mut self, term_event: TermEvent) {
        match term_event {
            TermEvent::Mouse(_) => (),
            TermEvent::Resize(_, _) => (),
            TermEvent::Key(KeyEvent { code, modifiers }) => match code {
                KeyCode::Esc => {
                    self.event_queue.sender().send_with_priority(Event::Close(None));
                }
                KeyCode::Char(character) => {
                    if character == 'c' && modifiers.contains(KeyModifiers::CONTROL) {
                        self.event_queue.sender().send_with_priority(Event::Close(None));
                    } else {
                        self.state.input_write(character);
                    }
                }
                KeyCode::Enter => {
                    if let Some(input) = self.state.reset_input() {
                        if input == "?stream" {
                            self.state.x = crate::state::Xstate::Streaming;
                        }
                        if input == "?stopstream" {
                            self.state.x = crate::state::Xstate::Idle;
                        }
                        match self.commands.find_command_action(&input).transpose() {
                            Ok(action) => {
                                let message = ChatMessage::new(
                                    format!("{} (me)", self.config.user_name),
                                    MessageType::Text(input.clone()),
                                );
                                self.state.add_message(message);

                                self.network.send_all(
                                    self.state.all_user_endpoints(),
                                    NetMessage::UserMessage(input.clone()),
                                );

                                if let Some(action) = action {
                                    self.process_action(action)
                                }
                            }
                            Err(error) => {
                                error.report_err(&mut self.state);
                            }
                        };
                    }
                }
                KeyCode::Delete => {
                    self.state.input_remove();
                }
                KeyCode::Backspace => {
                    self.state.input_remove_previous();
                }
                KeyCode::Left => {
                    self.state.input_move_cursor(CursorMovement::Left);
                }
                KeyCode::Right => {
                    self.state.input_move_cursor(CursorMovement::Right);
                }
                KeyCode::Home => {
                    self.state.input_move_cursor(CursorMovement::Start);
                }
                KeyCode::End => {
                    self.state.input_move_cursor(CursorMovement::End);
                }
                KeyCode::Up => {
                    self.state.messages_scroll(ScrollMovement::Up);
                }
                KeyCode::Down => {
                    self.state.messages_scroll(ScrollMovement::Down);
                }
                KeyCode::PageUp => {
                    self.state.messages_scroll(ScrollMovement::Start);
                }
                _ => (),
            },
        }
    }

    fn process_action(&mut self, mut action: Box<dyn Action>) {
        match action.process(&mut self.state, &mut self.network) {
            Processing::Completed => (),
            Processing::Partial => {
                self.event_queue.sender().send(Event::Action(action));
            }
        }
    }

    pub fn sender(&mut self) -> message_io::events::EventSender<Event> {
        self.event_queue.sender().clone()
    }
}
